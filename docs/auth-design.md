# jimeng-gateway 认证系统设计

> 目标：将 jimeng-gateway 从内部工具升级为**可分发 API 密钥的视频生成网关**，支持多调用方接入、用量追踪、速率限制。

## 1. 设计原则

| 原则 | 说明 |
|------|------|
| **零破坏** | 认证层作为 Axum middleware 插入，现有路由逻辑不改 |
| **最小依赖** | 不引入 JWT/OAuth 库；API Key + SHA256 足够简单可靠 |
| **SQLite 统一** | 所有状态（keys、usage、rate limits）存 SQLite，与现有 sessions/tasks 共库 |
| **渐进启用** | 环境变量 `AUTH_ENABLED=true` 开关，默认关闭兼容现有部署 |

## 2. 双轨认证模型

### 2.1 API Key（调用方）

```
格式：gw_<32 hex chars>   （共 35 字符）
存储：SHA256(key) → api_keys 表
传输：Authorization: Bearer gw_xxx
```

**生命周期**：Admin 在 Dashboard 创建 → 下发给调用方 → 调用方 Bearer 携带 → 网关校验

**API Key 属性**：

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | UUID |
| `name` | TEXT | 密钥名称（如 "StoryAI Desktop"、"客户A"） |
| `key_hash` | TEXT UNIQUE | SHA256(raw_key)，索引加速查询 |
| `key_prefix` | TEXT | 前 8 位明文（`gw_xxxx`）用于 Dashboard 展示 |
| `enabled` | BOOL | 是否启用 |
| `expires_at` | TEXT NULL | 过期时间（NULL = 永不过期） |
| `rate_limit` | INT | 每分钟最大请求数（0 = 不限） |
| `daily_quota` | INT | 每日最大任务数（0 = 不限） |
| `scopes` | TEXT | 权限范围 JSON，如 `["video:create","task:read"]` |
| `metadata` | TEXT | 自定义 JSON（联系信息、备注等） |
| `created_at` | TEXT | 创建时间 |
| `last_used_at` | TEXT | 最后使用时间 |

### 2.2 Admin Token（管理员）

两种方式，**优先级从高到低**：

1. **环境变量** `ADMIN_TOKEN=admin_xxxxx`：单一管理员 token，适合自部署
2. **API Key with admin scope**：`scopes` 包含 `"admin"` 的 key，适合多管理员

Admin 权限可访问：
- `/api/v1/sessions/*` — Session Pool CRUD
- `/api/v1/keys/*` — API Key CRUD
- `/api/v1/usage/*` — 用量统计
- `/api/v1/tasks/*` — 所有任务管理
- `/api/v1/logs/*` — Docker 日志

## 3. 数据库 Schema 变更

```sql
-- API Keys 表
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    key_prefix TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    expires_at TEXT,
    rate_limit INTEGER NOT NULL DEFAULT 60,   -- requests/min, 0=unlimited
    daily_quota INTEGER NOT NULL DEFAULT 0,   -- tasks/day, 0=unlimited
    scopes TEXT NOT NULL DEFAULT '["video:create","task:read","task:cancel"]',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);

-- 用量记录表（每 key 每天一行）
CREATE TABLE IF NOT EXISTS usage_daily (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL REFERENCES api_keys(id),
    date TEXT NOT NULL,                       -- YYYY-MM-DD
    request_count INTEGER NOT NULL DEFAULT 0, -- 总请求数
    task_count INTEGER NOT NULL DEFAULT 0,    -- 创建的任务数
    UNIQUE(api_key_id, date)
);
CREATE INDEX IF NOT EXISTS idx_usage_daily_key_date ON usage_daily(api_key_id, date);

-- tasks 表新增 api_key_id 列（追踪谁创建的）
ALTER TABLE tasks ADD COLUMN api_key_id TEXT;
```

## 4. Middleware 设计

### 4.1 认证提取器

```rust
// src/auth/extractor.rs

/// 认证后的调用者身份
pub enum Caller {
    /// API Key 调用方
    ApiKey {
        key_id: String,
        name: String,
        scopes: Vec<String>,
        rate_limit: u32,
        daily_quota: u32,
    },
    /// Admin（环境变量 token 或 admin scope key）
    Admin {
        source: AdminSource, // EnvToken | ApiKey(key_id)
    },
    /// 未认证（AUTH_ENABLED=false 时的降级模式）
    Anonymous,
}

pub enum AdminSource {
    EnvToken,
    ApiKey(String),
}
```

### 4.2 认证流程

```
请求进入
  │
  ├─ AUTH_ENABLED=false? → Caller::Anonymous → 放行
  │
  ├─ 提取 Authorization: Bearer <token>
  │   │
  │   ├─ 无 header → 401 Unauthorized
  │   │
  │   ├─ token == ADMIN_TOKEN → Caller::Admin(EnvToken) → 放行
  │   │
  │   ├─ SHA256(token) 查 api_keys 表
  │   │   │
  │   │   ├─ 未找到 → 401 Invalid API Key
  │   │   ├─ enabled=false → 403 Key Disabled
  │   │   ├─ expires_at < now → 403 Key Expired
  │   │   ├─ scopes 包含 "admin" → Caller::Admin(ApiKey)
  │   │   └─ 校验通过 → Caller::ApiKey
  │   │
  │   └─ Rate Limit 检查
  │       ├─ 超限 → 429 Too Many Requests
  │       └─ 通过 → Daily Quota 检查
  │           ├─ 超限（仅 task 创建接口）→ 429 Daily Quota Exceeded
  │           └─ 通过 → 记录 usage → 放行
  │
  └─ /ping, /v1/models → 免认证（健康检查）
```

### 4.3 速率限制

**Token Bucket 算法**（内存态，per API Key）：

```rust
pub struct RateLimiter {
    /// key_id → bucket
    buckets: DashMap<String, TokenBucket>,
}

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,     // = rate_limit
    refill_rate: f64,    // = rate_limit / 60.0 (tokens per second)
    last_refill: Instant,
}
```

- 使用 `dashmap` crate 做并发安全的内存 map
- 每次请求消耗 1 token；token 不足返回 429
- 响应头返回限流信息：`X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`

### 4.4 Scope 权限矩阵

| Scope | 可访问端点 | 说明 |
|-------|-----------|------|
| `video:create` | `POST /v1/videos/generations`, `POST /api/v1/tasks` | 创建视频任务 |
| `task:read` | `GET /api/v1/tasks`, `GET /api/v1/tasks/{id}`, `GET /api/v1/stats` | 查看任务状态 |
| `task:cancel` | `POST /api/v1/tasks/{id}/cancel` | 取消任务 |
| `task:retry` | `POST /api/v1/tasks/{id}/retry` | 重试任务 |
| `admin` | 所有端点（包括 sessions/keys/usage/logs CRUD） | 管理员全权限 |

默认新 key 的 scopes = `["video:create", "task:read", "task:cancel"]`

## 5. 新增 API 端点

### 5.1 API Key 管理（需 admin 权限）

```
POST   /api/v1/keys                  创建 API Key（返回明文 key，仅此一次）
GET    /api/v1/keys                  列出所有 Key（脱敏显示）
GET    /api/v1/keys/{id}             查看单个 Key 详情
PATCH  /api/v1/keys/{id}             更新 Key（name/enabled/rate_limit/daily_quota/scopes）
DELETE /api/v1/keys/{id}             删除 Key
POST   /api/v1/keys/{id}/regenerate  重新生成 Key（废弃旧 key，返回新明文）
```

#### 创建 Key 请求/响应

```json
// POST /api/v1/keys
// Request:
{
  "name": "StoryAI Desktop - 客户A",
  "rate_limit": 30,
  "daily_quota": 100,
  "scopes": ["video:create", "task:read", "task:cancel"],
  "expires_at": "2026-12-31T23:59:59Z",
  "metadata": { "contact": "a@example.com" }
}

// Response (201):
{
  "key": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "StoryAI Desktop - 客户A",
    "key": "gw_a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",  // ⚠️ 仅创建时返回明文
    "key_prefix": "gw_a1b2",
    "rate_limit": 30,
    "daily_quota": 100,
    "scopes": ["video:create", "task:read", "task:cancel"],
    "created_at": "2026-02-26T10:00:00Z"
  }
}
```

### 5.2 用量查询（需 admin 权限）

```
GET /api/v1/usage?key_id=xxx&from=2026-02-01&to=2026-02-28
GET /api/v1/usage/summary                 所有 key 汇总
```

```json
// Response:
{
  "usage": [
    {
      "date": "2026-02-26",
      "api_key_id": "550e...",
      "api_key_name": "StoryAI Desktop",
      "request_count": 156,
      "task_count": 42
    }
  ],
  "total": {
    "request_count": 1200,
    "task_count": 350
  }
}
```

### 5.3 调用方自查

```
GET /api/v1/me                     查看当前 key 信息（脱敏）+ 今日用量
```

```json
{
  "key": {
    "id": "550e...",
    "name": "StoryAI Desktop",
    "key_prefix": "gw_a1b2",
    "scopes": ["video:create", "task:read", "task:cancel"],
    "rate_limit": 30,
    "daily_quota": 100
  },
  "today": {
    "request_count": 42,
    "task_count": 12,
    "quota_remaining": 88
  }
}
```

## 6. 配置变更

### 6.1 新增环境变量

```env
# .env.example 新增
AUTH_ENABLED=false          # 是否启用认证（默认 false，兼容现有部署）
ADMIN_TOKEN=               # 管理员 token（留空则仅通过 API Key admin scope 认证）
```

### 6.2 Config 结构扩展

```rust
pub struct Config {
    // ... existing fields ...
    
    /// 是否启用 API Key 认证
    pub auth_enabled: bool,
    /// 管理员 token（环境变量直配，适合单管理员）
    pub admin_token: Option<String>,
}
```

## 7. 模块结构

```
src/
  auth/                    ← 新增认证模块
    mod.rs                 ← 公开导出
    middleware.rs           ← Axum middleware 层（提取 Bearer → 认证 → 注入 Caller）
    api_key.rs             ← API Key 生成 / 哈希 / 校验
    rate_limiter.rs        ← Token Bucket 速率限制器
    usage.rs               ← 用量记录与查询
  routes/
    keys.rs                ← 新增：API Key CRUD 端点
    usage.rs               ← 新增：用量查询端点
    me.rs                  ← 新增：调用方自查
    mod.rs                 ← 更新：注册新路由
    tasks.rs               ← 更新：enqueue 时关联 api_key_id
    compat.rs              ← 更新：enqueue 时关联 api_key_id
    sessions.rs            ← 不变（admin 权限保护）
    logs.rs                ← 不变（admin 权限保护）
  db/
    mod.rs                 ← 更新：新增 api_keys + usage_daily 迁移
  config.rs                ← 更新：新增 auth_enabled / admin_token
  main.rs                  ← 更新：初始化 auth 模块、插入 middleware
```

## 8. Cargo.toml 新增依赖

```toml
# 密码学（SHA256）
sha2 = "0.10"
hex = "0.4"

# 并发 HashMap（rate limiter）
dashmap = "6"

# 随机数生成（API Key）
rand = "0.8"
```

## 9. 与现有系统的集成

### 9.1 StoryAI Desktop 接入

当前 `generate_video` 工具通过 `JIMENG_API_URL` 直连 jimeng-api。迁移为网关后：

```
之前: JIMENG_API_URL=http://jimeng-api:8000  + JIMENG_SESSION_ID=xxx
之后: JIMENG_API_URL=https://gateway.example.com  + JIMENG_API_KEY=gw_xxx
```

StoryAI 的 `generate-video.ts` 只需将 `Authorization: Bearer {session_id}` 改为 `Authorization: Bearer {api_key}`，其余格式完全兼容（compat layer）。

### 9.2 Dashboard 适配

网关前端需新增：
- **API Keys 管理页**：创建/列表/编辑/删除/重新生成
- **用量统计页**：按 key 按天的请求/任务统计图表
- **登录页**（可选）：输入 admin token 后存 localStorage

## 10. 实施计划

| 阶段 | 内容 | 优先级 |
|------|------|--------|
| **P0** | `auth/` 模块 + api_keys 表 + middleware + `AUTH_ENABLED` 开关 | 核心 |
| **P0** | `routes/keys.rs` CRUD + key 生成逻辑 | 核心 |
| **P1** | Rate Limiter (Token Bucket) + 429 响应 | 高 |
| **P1** | Usage Tracker + `usage_daily` 表 + tasks.api_key_id 关联 | 高 |
| **P1** | Scope 检查 + Admin 权限隔离 | 高 |
| **P2** | Dashboard 前端：Keys 管理页 + 用量页 | 中 |
| **P2** | `/api/v1/me` 调用方自查 | 中 |
| **P3** | Key rotation / audit log | 低 |

## 11. 安全考量

1. **Key 明文不落盘**：数据库只存 SHA256 哈希，明文仅在创建时返回一次
2. **Admin Token 不泄露**：Dashboard 登录后存 sessionStorage（非 localStorage），关闭即清
3. **Rate Limit 防滥用**：即使 Key 泄露，速率限制兜底
4. **审计可追溯**：所有 task 关联 api_key_id，可追踪是谁创建的
5. **渐进启用**：`AUTH_ENABLED=false` 默认不影响现有部署
