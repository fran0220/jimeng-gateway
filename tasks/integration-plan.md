# 整合计划：jimeng-free-api-fork → jimeng-gateway

## 目标

消除 Node.js Docker 容器依赖，将即梦 API 交互逻辑直接用 Rust 实现，仅保留 chromium-browser 用于 Seedance a_bogus 签名。

## 当前架构

```
Client → jimeng-gateway (Rust:5100) → jimeng-free-api-all (Node.js Docker:8000) → jimeng.jianying.com
```

## 目标架构

```
Client → jimeng-gateway (Rust:5100) ─┬─ 普通API → reqwest → jimeng.jianying.com
                                      └─ Seedance generate → chromiumoxide (CDP) → jimeng.jianying.com
```

## 阶段一：修复现有 Gateway Bug（优先级 P0-P1）

### P0.1 Session 分配竞态条件
- **问题**：多个 worker 并发 `pick_session()` 读同一快照，可超配同一 session
- **修复**：原子化 pick+reserve，用 DB CAS 更新：
  ```sql
  UPDATE sessions SET active_tasks = active_tasks + 1, last_used_at = datetime('now')
  WHERE id = (SELECT id FROM sessions WHERE enabled=1 AND healthy=1 AND active_tasks < 2
              ORDER BY last_used_at LIMIT 1)
  RETURNING *
  ```

### P0.2 崩溃恢复缺失
- **问题**：重启后 tasks 卡在 submitting/polling，sessions.active_tasks 永久 >0
- **修复**：启动时执行：
  ```sql
  UPDATE sessions SET active_tasks = 0;
  UPDATE tasks SET status = 'queued' WHERE status IN ('submitting', 'polling', 'downloading')
    AND updated_at < datetime('now', '-10 minutes');
  ```

### P0.3 SQLite 写竞争 + 错误吞没
- **问题**：`let _ = sqlx::query(...)` 到处吞掉 DB 错误
- **修复**：
  - 配置 SQLite WAL 模式 + busy_timeout
  - 关键状态更新不吞错误，至少 tracing::warn

### P0.4 取消后仍写入 succeeded
- **问题**：worker success 路径不检查 cancelled 状态
- **修复**：`UPDATE tasks SET status='succeeded' WHERE id=? AND status != 'cancelled'`

### P1.1 "code unknown" 解析脆弱
- **问题**：只查 `fail_code` 一个字段，缺少则报 unknown
- **修复**：多字段容错解析，记录完整 payload

### P1.3 ECONNREFUSED 分类失效
- **问题**：`msg.to_lowercase()` 后用大写匹配 `"ECONNREFUSED"`
- **修复**：`msg.contains("econnrefused")`

### P1.4 非 JSON 响应崩溃
- **问题**：`response.json()` 直接 unwrap，上游返回 HTML/504 直接报错
- **修复**：先 `text()` 再 `serde_json::from_str()`

### P2.1 时间格式不一致
- **修复**：统一使用 `datetime('now')` 或 RFC3339

### P2.2 StatsRow SUM(NULL) → 500
- **修复**：`COALESCE(SUM(...), 0)`

### P2.3 fail_count 双重递增
- **修复**：移除内存中重复的 `s.fail_count += 1`

## 阶段二：移植即梦 API 直连（消除 Node.js 上游）

### 新增 Rust 模块

```
src/
├── jimeng/                    # 新增：即梦 API 直连
│   ├── mod.rs                # 公开接口
│   ├── auth.rs               # Cookie/Sign 伪装 (from core.ts)
│   ├── upload.rs             # ImageX + VOD 上传 (AWS4签名)
│   ├── models.rs             # 模型映射表 + 分辨率表
│   ├── submit.rs             # 任务提交 (普通视频 + Seedance)
│   ├── poll.rs               # 轮询结果 (get_history_by_ids)
│   └── browser.rs            # chromiumoxide CDP 浏览器代理
```

### 需移植的核心逻辑（from jimeng-free-api-fork）

| 来源 | 目标 | 说明 |
|------|------|------|
| `core.ts` generateCookie/Sign | `jimeng/auth.rs` | MD5签名、Cookie伪装 |
| `core.ts` request() | `jimeng/auth.rs` | 带签名的 HTTP 请求封装 |
| `videos.ts` MODEL_MAP | `jimeng/models.rs` | 模型名→内部名映射 |
| `videos.ts` VIDEO_RESOLUTION_OPTIONS | `jimeng/models.rs` | 分辨率表 |
| `videos.ts` createSignature (AWS4) | `jimeng/upload.rs` | ImageX/VOD 上传签名 |
| `videos.ts` uploadImageForVideo | `jimeng/upload.rs` | 图片上传到 ImageX |
| `videos.ts` uploadMediaForVideo | `jimeng/upload.rs` | 视频/音频上传到 VOD |
| `videos.ts` submitSeedanceVideo | `jimeng/submit.rs` | Seedance 提交逻辑 |
| `videos.ts` generateVideo | `jimeng/submit.rs` | 普通视频提交逻辑 |
| `browser-service.ts` | `jimeng/browser.rs` | chromiumoxide 替代 Playwright |

### 新依赖

```toml
# Cargo.toml 新增
chromiumoxide = "0.7"          # CDP 浏览器控制
md-5 = "0.10"                  # Sign 签名
crc32fast = "1"                # 文件 CRC32
hmac = "0.12"                  # AWS4 签名
```

### 服务器准备

```bash
ssh jpdata "apt install -y chromium-browser"
```

## 阶段三：清理

- 移除 `docker-compose.yml` 中 jimeng-free-api-all 服务
- 移除 `src/docker/` 模块（不再需要 Docker 日志流）
- 从 `config.rs` 移除 `jimeng_upstream` / `jimeng_container_name`
- 更新 systemd 服务文件
- 更新 CI/CD

## 工作量估算

| 阶段 | 工作量 | 说明 |
|------|--------|------|
| 阶段一：Bug 修复 | 1 天 | 全在 Rust 端改 |
| 阶段二：API 直连 | 3-4 天 | 核心移植工作 |
| 阶段三：清理 | 0.5 天 | 移除旧代码 |
| **总计** | **~5 天** | |
