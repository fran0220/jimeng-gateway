import { chromium, Browser, BrowserContext, Page } from "playwright-core";
import logger from "@/lib/logger.ts";
import { getCookiesForBrowser } from "@/api/controllers/core.ts";

// bdms SDK 相关脚本的白名单域名
const SCRIPT_WHITELIST_DOMAINS = [
  "vlabstatic.com",
  "bytescm.com",
  "jianying.com",
  "byteimg.com",
];

// 需要屏蔽的资源类型（加速加载、减少内存）
const BLOCKED_RESOURCE_TYPES = ["image", "font", "stylesheet", "media"];

// 会话空闲超时时间（毫秒）
const SESSION_IDLE_TIMEOUT = 10 * 60 * 1000;

// bdms SDK 就绪等待超时（毫秒）
const BDMS_READY_TIMEOUT = 30000;

interface BrowserSession {
  context: BrowserContext;
  page: Page;
  lastUsed: number;
  idleTimer: NodeJS.Timeout | null;
}

class BrowserService {
  private browser: Browser | null = null;
  private sessions: Map<string, BrowserSession> = new Map();
  private launching: Promise<Browser> | null = null;

  /**
   * 懒启动浏览器实例
   */
  private async ensureBrowser(): Promise<Browser> {
    if (this.browser?.isConnected()) {
      return this.browser;
    }

    // 防止并发启动
    if (this.launching) {
      return this.launching;
    }

    this.launching = (async () => {
      logger.info("BrowserService: 正在启动 Chromium 浏览器...");
      try {
        this.browser = await chromium.launch({
          headless: true,
          args: [
            "--no-sandbox",
            "--disable-setuid-sandbox",
            "--disable-dev-shm-usage",
            "--disable-gpu",
            "--no-first-run",
          ],
        });

        this.browser.on("disconnected", () => {
          logger.warn("BrowserService: 浏览器已断开连接");
          this.browser = null;
          this.sessions.clear();
        });

        logger.info("BrowserService: Chromium 浏览器启动成功");
        return this.browser;
      } finally {
        this.launching = null;
      }
    })();

    return this.launching;
  }

  /**
   * 获取或创建指定 token 的浏览器会话
   */
  private async getSession(token: string): Promise<BrowserSession> {
    const existing = this.sessions.get(token);
    if (existing) {
      existing.lastUsed = Date.now();
      // 重置空闲计时器
      if (existing.idleTimer) {
        clearTimeout(existing.idleTimer);
      }
      existing.idleTimer = setTimeout(() => this.closeSession(token), SESSION_IDLE_TIMEOUT);
      return existing;
    }

    return this.createSession(token);
  }

  /**
   * 创建新的浏览器会话
   */
  private async createSession(token: string): Promise<BrowserSession> {
    const browser = await this.ensureBrowser();

    logger.info(`BrowserService: 为 token ${token.substring(0, 8)}... 创建新会话`);

    const context = await browser.newContext({
      userAgent:
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36",
      viewport: { width: 1920, height: 1080 },
      locale: "zh-CN",
    });

    // 注入 cookies
    const cookies = getCookiesForBrowser(token);
    await context.addCookies(cookies);

    // 配置资源拦截
    await context.route("**/*", (route) => {
      const request = route.request();
      const resourceType = request.resourceType();
      const url = request.url();

      // 屏蔽不需要的资源类型
      if (BLOCKED_RESOURCE_TYPES.includes(resourceType)) {
        return route.abort();
      }

      // 对于脚本资源，只允许白名单域名
      if (resourceType === "script") {
        const isWhitelisted = SCRIPT_WHITELIST_DOMAINS.some((domain) =>
          url.includes(domain)
        );
        if (!isWhitelisted) {
          return route.abort();
        }
      }

      return route.continue();
    });

    const page = await context.newPage();

    // 导航到即梦页面，让 bdms SDK 加载
    logger.info("BrowserService: 正在导航到 jimeng.jianying.com ...");
    await page.goto("https://jimeng.jianying.com", {
      waitUntil: "domcontentloaded",
      timeout: 30000,
    });

    // 等待 bdms SDK 就绪
    logger.info("BrowserService: 等待 bdms SDK 就绪...");
    try {
      await page.waitForFunction(
        () => {
          // bdms SDK 会替换 window.fetch，检测其是否被替换
          // 也可以检测 window.bdms 或 window.byted_acrawler
          return (
            (window as any).bdms?.init ||
            (window as any).byted_acrawler ||
            // 检测 fetch 是否被替换（bdms 会替换原生 fetch）
            window.fetch.toString().indexOf("native code") === -1
          );
        },
        { timeout: BDMS_READY_TIMEOUT }
      );
      logger.info("BrowserService: bdms SDK 已就绪");
    } catch (err) {
      logger.warn(
        "BrowserService: bdms SDK 等待超时，可能未完全加载，继续尝试..."
      );
    }

    const session: BrowserSession = {
      context,
      page,
      lastUsed: Date.now(),
      idleTimer: setTimeout(() => this.closeSession(token), SESSION_IDLE_TIMEOUT),
    };

    this.sessions.set(token, session);
    return session;
  }

  /**
   * 关闭指定 token 的会话
   */
  private async closeSession(token: string) {
    const session = this.sessions.get(token);
    if (!session) return;

    logger.info(`BrowserService: 关闭空闲会话 ${token.substring(0, 8)}...`);
    if (session.idleTimer) {
      clearTimeout(session.idleTimer);
    }

    try {
      await session.context.close();
    } catch (err) {
      // 忽略关闭错误
    }

    this.sessions.delete(token);
  }

  /**
   * 通过浏览器代理发送 fetch 请求
   * bdms SDK 会自动拦截 fetch 并注入 a_bogus 签名
   *
   * @param token sessionid
   * @param url 完整的请求 URL
   * @param options fetch 选项 (method, headers, body)
   * @returns 解析后的 JSON 响应
   */
  async fetch(
    token: string,
    url: string,
    options: { method?: string; headers?: Record<string, string>; body?: string }
  ): Promise<any> {
    const session = await this.getSession(token);

    logger.info(`BrowserService: 代理请求 ${options.method || "GET"} ${url.substring(0, 100)}...`);

    try {
      const result = await session.page.evaluate(
        async ({ url, options }) => {
          try {
            const res = await fetch(url, {
              method: options.method || "GET",
              headers: {
                "Content-Type": "application/json",
                ...(options.headers || {}),
              },
              body: options.body,
              credentials: "include",
            });
            const text = await res.text();
            return { ok: res.ok, status: res.status, text };
          } catch (err: any) {
            return { ok: false, status: 0, text: "", error: err.message };
          }
        },
        { url, options }
      );

      if (result.error) {
        throw new Error(`浏览器 fetch 失败: ${result.error}`);
      }

      logger.info(`BrowserService: 响应状态 ${result.status}`);

      try {
        return JSON.parse(result.text);
      } catch {
        logger.warn(`BrowserService: 响应不是有效 JSON: ${result.text.substring(0, 200)}`);
        return result.text;
      }
    } catch (err) {
      // 如果执行失败（页面崩溃等），清理会话以便下次重建
      logger.error(`BrowserService: 请求执行失败: ${(err as Error).message}`);
      await this.closeSession(token);
      throw err;
    }
  }

  /**
   * 关闭所有会话和浏览器实例
   */
  async close() {
    logger.info("BrowserService: 正在关闭所有会话和浏览器...");

    for (const [token] of this.sessions) {
      await this.closeSession(token);
    }

    if (this.browser) {
      try {
        await this.browser.close();
      } catch (err) {
        // 忽略关闭错误
      }
      this.browser = null;
    }

    logger.info("BrowserService: 已关闭");
  }
}

// 单例导出
const browserService = new BrowserService();
export default browserService;
