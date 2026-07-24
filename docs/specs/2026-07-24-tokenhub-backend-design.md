# TokenHub 后端服务设计文档

- **版本**：v1.0
- **日期**：2026-07-24
- **状态**：已评审 / 待实现
- **技术栈**：Rust (cargo workspace) + SQLite + Redis + Docker
- **不提交 git**（按项目约定，仅本地落盘）

---

## 1. 系统概览与目标

TokenHub 是面向企业级 LLM 应用的基础设施服务（AI Gateway），提供与 OpenAI API 兼容的统一接口，聚合管理多个模型供应商（OpenAI、Anthropic、Gemini、自部署模型等），并负责身份验证、调用配额控制、积分计费、审计与监控。

### 核心目标

| 目标 | 说明 |
|---|---|
| 统一 API 接口 | 下游应用通过 `/v1/chat/completions` 等 OpenAI 风格接口调用，不感知后端模型细节 |
| 多模型池化 | 供应商被抽象为逻辑模型（basic/standard/expert），内部按分级路由 + 负载均衡 + 自动降级 |
| 资源计费 | 统一**积分（Credit）体系**，按模型汇率计量消耗，实时原子扣减 |
| 身份鉴权 | 区分 管理员(Admin) / 服务系统(Service) / 应用用户(API用户) 三类主体 |
| 成本监控 | 按供应商池子监控用量，80% 阈值告警，额度耗尽自动切换 |

### 规模定位

**MVP / 原型规模**：单机部署、数十~数百账号、中等 QPS。SQLite 作为主存储（通过 WAL + 乐观锁支持并发扣费），Redis 作为热点缓存层。存储层通过 Repository trait 抽象，未来可平滑迁移到 PostgreSQL。

---

## 2. 整体架构

### 2.1 部署架构

```
                        公网
                          │
                          ▼
              https://<random>.tun.alius.tech
                          │
                  ┌───────┴────────┐
                  │  Caddy (已有)   │  ← TLS 终结, 自动 HTTPS
                  └───────┬────────┘
                          │ 内网 HTTP
                          ▼
              ┌───────────────────────┐
              │  frps @ frp.alius.tech │  ← 独立服务（已搭建）
              └───────────┬───────────┘
                          │ frp 隧道
                          ▼
   ┌──────────────────────────────────────────────┐
   │           docker-compose 编排                 │
   │  ┌────────────┐    ┌──────────────────────┐  │
   │  │  tun 容器   │◄──►│  TokenHub 容器        │  │
   │  │ (frpc)     │    │  :8080 chat (内网)    │  │
   │  │            │    │  :8081 admin          │  │
   │  └────────────┘    └──────────┬───────────┘  │
   └───────────────────────────────┼──────────────┘
                                   ▼
                          ┌──────────────┐
                          │  Redis 容器   │ ← 独立服务
                          └──────────────┘
                                   │
                          ┌────────▼─────────┐
                          │ 上游 LLM 供应商   │
                          │ OpenAI/Claude/...│
                          └──────────────────┘
```

### 2.2 双端口设计

单容器、单进程、双 axum listener，共享同一 `AppState`：

| 端口 | 用途 | 鉴权主体 | 暴露方式 |
|---|---|---|---|
| **:8080** Chat API | 面向应用/最终用户 | API Token | 公网/内网（不走 tun） |
| **:8081** Admin API | 面向管理员/服务系统 | Admin Token / Service JWT | 仅通过 tun 远程接入 |

两个 router 装载不同的鉴权中间件栈。Admin 端在生产建议只绑内网 / 走 tun，不直接映射公网。

### 2.3 Redis 角色（缓存层，非权威）

| 数据 | 入 Redis | 用途 |
|---|---|---|
| API Token → account 映射 | ✅ | 每请求鉴权热点，避免打 SQLite |
| 账号余额 | ✅ | 前置快速预筛（余额不足直接拒绝）+ 热读展示 |
| 账号权限 / scope | ✅ | 随 token 映射一起缓存 |
| 模型汇率 | ✅ | 每次计费要读，很少变 |
| 限流计数 | ✅ | per-token / per-IP 滑动窗口 |

**权威数据永远在 SQLite**。Redis 仅缓存，重启不影响正确性（最坏回源一次）。

---

## 3. 身份认证与鉴权

### 3.1 三类身份

| 主体 | 认证方式 | Token 形态 | 可吊销 |
|---|---|---|---|
| **Admin（管理员）** | 手机号 + 密码 + TOTP | 有状态 access token（DB 记录 + TTL） | ✅ 即时 |
| **Service（服务系统）** | OAuth2 client_credentials | 短期 JWT（含 scope） | 靠短 TTL |
| **API 用户（应用）** | 静态 Bearer Token | 长生命周期 API Token（DB 存 HMAC hash） | ✅ 删 token 清缓存 |

### 3.2 Admin 登录（TOTP 为主）

- 首个管理员通过 CLI `tokenhub admin create` 初始化（绑定 TOTP secret + 密码）。
- 登录：`phone + password + totp_code` → 校验 → 颁发**有状态 access token**（随机串写入 DB，带过期时间）。
- 选择有状态（非纯 JWT）：满足"立即吊销"需求（logout / 凭证泄露）。
- 短信验证码作为**可选增强**（`Notifier` trait，MVP 用 Console/Log 实现）。

### 3.3 Service 认证（OAuth2 Client Credentials）

- 管理员创建 Service，生成 `client_id` + `client_secret`（明文仅返回一次，DB 存 hash）。
- Service 调用 `POST /v1/admin/auth/token`（grant_type=client_credentials）换取短期 JWT。
- JWT 含 `iss=client_id`、`exp`、`scope`，用对称密钥（HMAC）或非对称密钥签名。
- 比 RSA 私钥签名集成成本低，比静态 Key 安全（短生命周期 + 可吊销 secret）。
- RSA 私钥签名方案保留为未来增强（`service_accounts` 表预留 `public_key` 字段）。

### 3.4 API Token（应用用户）

- 管理员/Service 为账号创建 API Token，明文仅返回一次。
- **DB 存 `HMAC-SHA256(token, server_secret)`**（不用慢哈希，每请求一次会拖垮性能）。
- 每请求：HMAC(收到的 token) → Redis 查 `token:<hmac>` → 命中拿 account + scope。
- 未命中 → 回源 SQLite → 回填 Redis（TTL 10m）。
- 吊销 = 删 DB 记录 + `DEL token:<hmac>`（即时失效）。
- Token 前缀（明文前 8 位）存 DB，用于前端识别展示。

### 3.5 统一 Principal 模型

三类主体最终都映射到统一的 `Principal { kind, id, scopes }`，在鉴权中间件统一校验 scope。Admin 和 Service 都调用 `/v1/admin/*`，区别仅在凭证形式和 scope。

---

## 4. 模型池分级路由

### 4.1 分级模型

- 同一逻辑模型名（`basic`/`standard`/`expert`）下聚合多个供应商实例。
- **每个供应商实例标记所属级别**（level 1/2/3），而非整个逻辑模型一个级别。
- 从各厂家拉取真实模型列表后，运营在后台给每个模型标注 level。

### 4.2 调用策略（可配置）

| 策略 | 行为 |
|---|---|
| `sequential` | 按 level 升序逐个尝试，直到成功 |
| `random` | 在同级或全部候选中按 weight 加权随机 |

### 4.3 额度耗尽自动降级

- 调用某供应商返回 429（限流）/ 402（欠费）/ 配额耗尽类错误 → **立即标记该供应商 token 为 `disabled`**（带原因 + 时间戳）。
- 自动 fallback 到下一个候选模型，对调用方透明。
- **`disabled` 的 token 不自动恢复**，必须管理员介入（DELETE 或更新凭证）。

### 4.4 供应商额度监控（80% 告警）

- `provider_credentials` 配 `quota_limit` + `quota_threshold`（默认 80）。
- 每次成功调用累加 `quota_used`（平台累加，基于响应 usage）。
- `quota_used / quota_limit >= 阈值` 且 `alert_sent=false` → 触发 Notifier 告警，置 `alert_sent=true`（防重复）。
- 定时任务（每小时/每天）调用官方 Usage API 对账，修正累加误差。

### 4.5 熔断（内存态）

- 每个 provider 维护滑动窗口错误率，超阈值临时摘除（MVP 单实例用内存；多实例需上 Redis）。

---

## 5. 积分计量与计费

### 5.1 积分是唯一权威货币

- 账户表只存 `balance`（积分，`i64`），不存 token 数。
- 上游返回 token 数 → 乘该模型汇率 → 得积分 → 扣 balance。
- **整数运算**（全程 `i64`），汇率配置成"每 1000 token = N 积分"：`credits = tokens * rate_per_1k / 1000`。

### 5.2 计量基准：usage 权威，tiktoken 兜底

| 场景 | tiktoken 角色 | 影响最终计费 |
|---|---|---|
| 调用前预冻结 | 估算输入 token，决定冻结额度 | ❌ 不影响（最终按 usage 结算） |
| 上游无 usage 兜底 | 估算输入+输出 token，作为计费依据 | ✅ 影响（标记 `tiktoken_fallback` 可修正） |

- **主路径（>95%）**：上游响应含 usage → 直接用官方数字计费（所有主流供应商都返回）。
- **兜底**：自部署/小众模型不返回 usage → tiktoken 估算，`usage_source = "tiktoken_fallback"`。
- tiktoken 对非 OpenAI 模型有偏差，但兜底是低频容错路径，加保守系数防少扣，标记可对账。

### 5.3 预冻结 → 结算流程（并发安全核心）

```
请求到达
  │
  ├─[1] 鉴权：Redis 查 token:<hmac> → account
  │
  ├─[2] 余额预筛：Redis 读 balance → <门槛则 402（挡无效请求）
  │
  ├─[3] 预冻结（SQLite 乐观锁）：
  │     tiktoken 估算 prompt → 估算积分
  │     UPDATE credits SET balance=balance-:est, held=held+:est, version=version+1
  │       WHERE account_id=? AND balance>=:est AND version=:v
  │     affected=0 → 并发冲突/不足 → 重试 N 次 → 仍失败 402
  │
  ├─[4] 调用上游 LLM（按 level + 策略路由，失败自动降级）
  │
  ├─[5] 结算（SQLite 乐观锁）：
  │     usage 计算实际积分（或 tiktoken 兜底）
  │     UPDATE credits SET held=held-:est, balance=balance-(:actual-:est), version=version+1
  │       WHERE account_id=? AND version=:v2
  │     RETURNING balance → 回填 Redis
  │     事务内同时更新：用户预聚合 + 供应商预聚合 + 供应商额度累加
  │
  └─[6] 异步写 usage_logs（mpsc channel 批量 INSERT）
```

**为什么不会超卖**：SQLite 乐观锁 UPDATE 是权威扣减，并发请求靠 `version` 串行化。Redis 余额仅用于前置预筛，即使偏旧，乐观锁那步会挡住真正不够的请求——最坏误判一次重试，绝不超卖。

### 5.4 多维度统计

一次请求的 usage，同时服务收入侧和成本侧：

```
usage_logs（事实表，每请求一条）
  account_id  ← 收入侧维度（谁调的）
  provider_id ← 成本侧维度（打到哪个池子）
```

- 按用户聚合（收入侧）：该用户扣了多少积分、调了多少次。
- 按池子聚合（成本侧）：该供应商共消耗多少、是否到 80%。
- 用**预聚合表**（`account_usage_summary` / `provider_usage_summary`）UPSERT 累加，事务内与扣费同步更新，避免扫描日志。

---

## 6. 完整接口清单（60 个）

### 6.1 Chat API（:8080，OpenAI 兼容，API Token 鉴权）

| # | 方法 | 路径 | 用途 |
|---|---|---|---|
| 1 | POST | `/v1/chat/completions` | 聊天生成（支持 stream SSE） |
| 2 | GET | `/v1/models` | 当前账号可用逻辑模型列表 |
| 3 | GET | `/v1/usage` | 当前账号余额 + 用量统计 |
| 4 | GET | `/healthz` | 容器健康检查 |

### 6.2 Admin API（:8081）

**认证**

| # | 方法 | 路径 | 鉴权 | 用途 |
|---|---|---|---|---|
| 5 | POST | `/v1/admin/auth/login` | 无 | 管理员登录（phone+password+totp） |
| 6 | POST | `/v1/admin/auth/refresh` | Refresh Token | 刷新 Access Token |
| 7 | POST | `/v1/admin/auth/logout` | Admin Token | 登出（吊销） |
| 8 | GET | `/v1/admin/auth/me` | Admin Token | 当前管理员信息 |
| 9 | POST | `/v1/admin/auth/token` | client_id+secret | Service 换 JWT（client_credentials） |

**管理员管理**

| # | 方法 | 路径 | scope | 用途 |
|---|---|---|---|---|
| 10 | GET | `/v1/admin/admins` | admin.read | 管理员列表 |
| 11 | POST | `/v1/admin/admins` | admin.write | 创建管理员 |
| 12 | PUT | `/v1/admin/admins/{id}` | admin.write | 修改管理员 |
| 13 | DELETE | `/v1/admin/admins/{id}` | admin.write | 停用管理员 |
| 14 | POST | `/v1/admin/admins/{id}/reset-totp` | admin.write | 重置 TOTP |

**账号管理**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 15 | GET | `/v1/admin/accounts` | accounts.read |
| 16 | POST | `/v1/admin/accounts` | accounts.write |
| 17 | GET | `/v1/admin/accounts/{id}` | accounts.read |
| 18 | PUT | `/v1/admin/accounts/{id}` | accounts.write |
| 19 | DELETE | `/v1/admin/accounts/{id}` | accounts.write |

**API Token 管理**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 20 | GET | `/v1/admin/tokens` | tokens.read |
| 21 | POST | `/v1/admin/tokens` | tokens.write |
| 22 | DELETE | `/v1/admin/tokens/{id}` | tokens.write |

**积分管理**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 23 | GET | `/v1/admin/credits` | credits.read |
| 24 | POST | `/v1/admin/credits` | credits.write |
| 25 | PUT | `/v1/admin/credits/{account_id}` | credits.admin |
| 26 | GET | `/v1/admin/credits/transactions` | credits.read |

**模型与汇率**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 27 | GET | `/v1/admin/models` | models.read |
| 28 | POST | `/v1/admin/models` | models.write |
| 29 | PUT | `/v1/admin/models/{id}` | models.write |
| 30 | DELETE | `/v1/admin/models/{id}` | models.write |
| 31 | PUT | `/v1/admin/models/{id}/rates` | models.write |

**供应商凭证**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 32 | GET | `/v1/admin/providers` | providers.read |
| 33 | POST | `/v1/admin/providers` | providers.write |
| 34 | PUT | `/v1/admin/providers/{id}` | providers.write |
| 35 | DELETE | `/v1/admin/providers/{id}` | providers.write |
| 36 | POST | `/v1/admin/providers/{id}/disable` | providers.write |
| 37 | POST | `/v1/admin/providers/{id}/enable` | providers.write |
| 38 | GET | `/v1/admin/providers/{id}/usage` | providers.read |
| 39 | POST | `/v1/admin/providers/{id}/sync-quota` | providers.write |

**模型-供应商映射 + 上游模型同步**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 40 | GET | `/v1/admin/model-providers` | models.read |
| 41 | POST | `/v1/admin/model-providers` | models.write |
| 42 | PUT | `/v1/admin/model-providers/{id}` | models.write |
| 43 | DELETE | `/v1/admin/model-providers/{id}` | models.write |
| 44 | POST | `/v1/admin/providers/{id}/sync-models` | providers.write |
| 45 | GET | `/v1/admin/upstream-models` | models.read |
| 46 | PUT | `/v1/admin/upstream-models/{id}/label` | models.write |

**Service 账号**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 47 | GET | `/v1/admin/services` | services.read |
| 48 | POST | `/v1/admin/services` | services.write |
| 49 | PUT | `/v1/admin/services/{id}` | services.write |
| 50 | DELETE | `/v1/admin/services/{id}` | services.write |
| 51 | POST | `/v1/admin/services/{id}/reset-secret` | services.write |

**策略模板**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 52 | GET | `/v1/admin/policies` | policies.read |
| 53 | POST | `/v1/admin/policies` | policies.write |
| 54 | PUT | `/v1/admin/policies/{id}` | policies.write |
| 55 | DELETE | `/v1/admin/policies/{id}` | policies.write |
| 56 | POST | `/v1/admin/accounts/{id}/policy` | policies.write |

**报表与审计**

| # | 方法 | 路径 | scope |
|---|---|---|---|
| 57 | GET | `/v1/admin/reports/usage` | reports.read |
| 58 | GET | `/v1/admin/reports/cost` | reports.read |
| 59 | GET | `/v1/admin/audit/logs` | audit.read |
| 60 | GET | `/v1/admin/usage-logs` | reports.read |

---

## 7. 数据模型

> 约定：主键统一 TEXT（UUID/ULID 字符串）；时间戳存 INTEGER（Unix 毫秒）；启用 `PRAGMA foreign_keys=ON` + WAL + `busy_timeout`。所有外键建索引。

### 7.1 核心表 DDL（摘要）

```sql
-- 管理员
CREATE TABLE admin_users (
  id TEXT PRIMARY KEY,
  phone TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  totp_secret TEXT NOT NULL,
  roles TEXT NOT NULL,           -- JSON array
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  last_login_at INTEGER
);

-- 管理员有状态 access token（可吊销）
CREATE TABLE admin_access_tokens (
  token_hash TEXT PRIMARY KEY,   -- HMAC(token)
  admin_id TEXT NOT NULL REFERENCES admin_users(id),
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_admin_tokens_admin ON admin_access_tokens(admin_id);

-- refresh token
CREATE TABLE admin_refresh_tokens (
  token_hash TEXT PRIMARY KEY,
  admin_id TEXT NOT NULL REFERENCES admin_users(id),
  access_token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);

-- 账号（应用用户/业务账号）
CREATE TABLE accounts (
  id TEXT PRIMARY KEY,
  external_id TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  policy_id TEXT REFERENCES policies(id),
  note TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX idx_accounts_external ON accounts(external_id);

-- API Token（DB 存 HMAC hash）
CREATE TABLE api_tokens (
  id TEXT PRIMARY KEY,
  token_hash TEXT UNIQUE NOT NULL,  -- HMAC-SHA256
  prefix TEXT NOT NULL,             -- 明文前 8 位
  account_id TEXT NOT NULL REFERENCES accounts(id),
  name TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  expires_at INTEGER,
  created_at INTEGER NOT NULL,
  revoked_at INTEGER
);
CREATE INDEX idx_tokens_account ON api_tokens(account_id);

-- Service 账号
CREATE TABLE service_accounts (
  id TEXT PRIMARY KEY,
  client_id TEXT UNIQUE NOT NULL,
  client_secret_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  scopes TEXT NOT NULL,            -- JSON array
  ip_whitelist TEXT,               -- JSON array of CIDR
  public_key TEXT,                 -- 预留：未来 RSA 签名
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 逻辑模型
CREATE TABLE models (
  id TEXT PRIMARY KEY,
  logical_name TEXT UNIQUE NOT NULL,  -- expert/standard/basic
  description TEXT,
  input_rate_per_1k INTEGER NOT NULL,  -- 每 1k input token 的积分
  output_rate_per_1k INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 供应商凭证
CREATE TABLE provider_credentials (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  provider_type TEXT NOT NULL,       -- openai/anthropic/gemini/custom
  base_url TEXT NOT NULL,
  api_key_enc TEXT NOT NULL,          -- 加密存储
  status TEXT NOT NULL DEFAULT 'active',  -- active/disabled
  disabled_reason TEXT,
  disabled_at INTEGER,
  quota_limit INTEGER,                -- 额度上限（按官方货币/token）
  quota_used INTEGER NOT NULL DEFAULT 0,
  quota_threshold INTEGER NOT NULL DEFAULT 80,
  quota_alert_sent INTEGER NOT NULL DEFAULT 0,
  quota_synced_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 逻辑模型 ↔ 供应商映射
CREATE TABLE model_providers (
  id TEXT PRIMARY KEY,
  logical_model_id TEXT NOT NULL REFERENCES models(id),
  provider_id TEXT NOT NULL REFERENCES provider_credentials(id),
  upstream_model TEXT NOT NULL,       -- 如 gpt-4o / claude-3-5-sonnet
  level INTEGER NOT NULL,             -- 1/2/3
  weight INTEGER NOT NULL DEFAULT 100,
  strategy TEXT NOT NULL DEFAULT 'sequential',  -- sequential/random
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);

-- 上游模型（拉取的厂商真实模型，待标注）
CREATE TABLE upstream_models (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL REFERENCES provider_credentials(id),
  upstream_model TEXT NOT NULL,
  level INTEGER,                      -- 标注后填
  labeled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

-- 积分余额（乐观锁）
CREATE TABLE credits (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id),
  balance INTEGER NOT NULL DEFAULT 0,
  held INTEGER NOT NULL DEFAULT 0,    -- 冻结额
  version INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);

-- 积分流水
CREATE TABLE credit_transactions (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  delta INTEGER NOT NULL,             -- 正充值/负扣减
  balance_after INTEGER NOT NULL,
  reason TEXT,
  operator TEXT,                      -- admin id / service id / 'system'
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_credit_tx_account ON credit_transactions(account_id, created_at);

-- 预冻结（hold）
CREATE TABLE credit_holds (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  amount INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'held',  -- held/settled/released
  request_id TEXT,                      -- 关联调用请求
  created_at INTEGER NOT NULL,
  settled_at INTEGER
);
CREATE INDEX idx_holds_account ON credit_holds(account_id, status);

-- 调用日志（事实表）
CREATE TABLE usage_logs (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  logical_model TEXT NOT NULL,
  provider_id TEXT,
  upstream_model TEXT,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  total_tokens INTEGER,
  credits_cost INTEGER NOT NULL,
  usage_source TEXT NOT NULL,         -- upstream/tiktoken_fallback
  status TEXT NOT NULL,               -- success/failed/fallback
  source_ip TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_usage_account_time ON usage_logs(account_id, created_at);
CREATE INDEX idx_usage_provider_time ON usage_logs(provider_id, created_at);

-- 用户用量预聚合
CREATE TABLE account_usage_summary (
  account_id TEXT NOT NULL,
  period TEXT NOT NULL,               -- yyyymm
  prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  credits INTEGER NOT NULL DEFAULT 0,
  calls INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, period)
);

-- 供应商用量预聚合
CREATE TABLE provider_usage_summary (
  provider_id TEXT NOT NULL,
  period TEXT NOT NULL,
  tokens_used INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (provider_id, period)
);

-- 操作审计日志
CREATE TABLE audit_logs (
  id TEXT PRIMARY KEY,
  actor_kind TEXT NOT NULL,           -- admin/service/system
  actor_id TEXT,
  action TEXT NOT NULL,               -- rate.update/provider.disable/...
  target_type TEXT,
  target_id TEXT,
  detail TEXT,                        -- JSON 变更前后
  source_ip TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_audit_time ON audit_logs(created_at);

-- 策略模板
CREATE TABLE policies (
  id TEXT PRIMARY KEY,
  name TEXT UNIQUE NOT NULL,
  allowed_models TEXT NOT NULL,       -- JSON array
  monthly_credit_cap INTEGER,
  description TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

---

## 8. 部署架构（docker-compose）

### 8.1 服务编排

三个 service：`tokenhub`（主）、`tun`（frpc 接入）、`redis`（缓存）。同一 docker network。

```yaml
services:
  tokenhub:
    build: .
    environment:
      - DATABASE_URL=sqlite:///data/tokenhub.db
      - REDIS_URL=redis://redis:6379
      - CHAT_LISTEN=0.0.0.0:8080
      - ADMIN_LISTEN=0.0.0.0:8081
      - SERVER_SECRET=${SERVER_SECRET}
    volumes:
      - tokenhub-data:/data
    depends_on: [redis]
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://localhost:8080/healthz"]
    restart: unless-stopped

  tun:
    image: snowdreamtech/frpc:latest
    volumes:
      - ./tun/tun.toml:/etc/frp/frpc.toml:ro
    environment:
      - TUN_SERVER=${TUN_SERVER:-frp.alius.tech}
      - TUN_PORT=${TUN_PORT:-7000}
      - TUN_TOKEN=${TUN_TOKEN:-}
      - TUN_SUBDOMAIN=${TUN_SUBDOMAIN}
      - TUN_LOCAL_ADDR=tokenhub
      - TUN_LOCAL_PORT=8081
    depends_on: [tokenhub]
    restart: unless-stopped

  redis:
    image: redis:7-alpine
    volumes:
      - redis-data:/data
    restart: unless-stopped

volumes:
  tokenhub-data:
  redis-data:
```

### 8.2 tun 接入配置

`tun/tun.toml`（entrypoint 脚本注入环境变量）：

```toml
serverAddr = "${TUN_SERVER}"
serverPort = ${TUN_PORT}
# auth.token = "${TUN_TOKEN}"

[[proxies]]
name = "tokenhub-admin"
type = "http"
localIP = "${TUN_LOCAL_ADDR}"
localPort = ${TUN_LOCAL_PORT}
customDomain = "${TUN_SUBDOMAIN}.tun.alius.tech"
```

- 子域名结构：`<random>.tun.alius.tech`
- 只暴露 :8081（Admin），Chat 不走 tun
- TLS 由 Caddy 终结，tokenhub 与 tun 之间走内网 HTTP
- **对外文档称"远程管理接入"，不出现 frp 字样**；frp 仅出现在内部实现备注

### 8.3 `.env.example`

```
# === 接入网关 (tun) 配置 ===
TUN_SERVER=frp.alius.tech
TUN_PORT=7000
TUN_SUBDOMAIN=change-me          # 随机服务 ID
TUN_TOKEN=                       # frps 开启 token 鉴权时填写

# === TokenHub 配置 ===
SERVER_SECRET=change-me-please
DATABASE_URL=sqlite:///data/tokenhub.db
REDIS_URL=redis://redis:6379
CHAT_LISTEN=0.0.0.0:8080
ADMIN_LISTEN=0.0.0.0:8081

# === Admin CLI 初始化 ===
# 首次部署用 docker run 覆盖 entrypoint 执行 tokenhub admin create
```

---

## 9. 安全性与风险防范

| 维度 | 措施 |
|---|---|
| 传输安全 | Caddy 强制 HTTPS；内网段明文可接受 |
| 密钥存储 | API Token 存 HMAC hash（不存明文）；供应商 api_key 加密存；Service secret 存 hash |
| 密钥轮换 | Service 支持重置 secret；供应商凭证支持轮换；旧凭证即时失效 |
| Token 吊销 | API Token 删记录+清 Redis；Admin token 有状态可即时吊销 |
| 访问控制 | 统一 Principal + scope 校验；Admin 接口最小权限；Admin 端只走 tun 不暴露公网 |
| 并发计费 | 乐观锁保证不超卖；余额不足返回 402 |
| 注入防护 | sqlx 参数化查询；输入校验 |
| 限流防滥用 | per-token / per-IP 滑动窗口限流；高频失败临时冻结 |
| 日志脱敏 | 日志不记录完整 token/密钥；只记前缀 |
| 外部依赖隔离 | 上游 LLM 故障不拖垮平台（超时/降级/熔断）；短信服务故障不影响 TOTP 登录 |
| tun 安全 | 随机子域名不可作安全手段；Admin 强鉴权（TOTP+有状态 token）；建议开启 frps auth.token；可选 Admin IP 白名单 |

---

## 10. 技术选型与项目结构

### 10.1 技术栈

| 层 | 选型 |
|---|---|
| 语言 | Rust (edition 2021) |
| Web 框架 | axum + tower / tower-http |
| 异步运行时 | tokio |
| HTTP 客户端 | reqwest (rustls) |
| 数据库 | sqlx (sqlite, runtime-tokio, rustls)，编译期 SQL 校验 |
| 迁移 | sqlx migrate |
| 序列化 | serde + serde_json |
| 认证加密 | jsonwebtoken、argon2、sha2、hmac、totp-rs |
| 配置 | config + 环境变量 + TOML |
| 日志 | tracing + tracing-subscriber |
| 错误 | thiserror + anyhow |
| ID | uuid v4 / ULID |
| 分词 | tiktoken-rs |
| 限流 | 自实现令牌桶（Redis 滑动窗口） |

### 10.2 cargo workspace 划分

```
token_hub/
├── Cargo.toml                  # [workspace]
├── crates/
│   ├── domain/                 # 领域模型 struct/enum
│   ├── storage/                # Repository trait + sqlx SQLite 实现 + 迁移
│   ├── cache/                  # Redis 封装（token/余额/汇率/限流）
│   ├── auth/                   # TOTP/JWT/API Token HMAC/有状态 session
│   ├── billing/                # 汇率/预冻结/结算/tiktoken
│   ├── router-llm/             # 分级路由/上游代理/SSE/降级/熔断
│   ├── audit/                  # 审计 + mpsc 批量写
│   ├── api/                    # axum 路由 + handler + 中间件（双 router）
│   └── cli/                    # tokenhub admin create / models sync
├── src/main.rs                 # 组装 + 配置 + 双 listener
├── migrations/                 # sqlx 迁移 SQL
├── tun/tun.toml                # frpc 配置模板
├── Dockerfile / docker-compose.yml / .env.example
└── tests/                      # 集成测试
```

---

## 11. 开发计划概览（详见开发计划文档）

| Phase | 内容 | 里程碑 |
|---|---|---|
| Phase 0 | 脚手架 + Docker + healthz | - |
| Phase 1 | 存储层 + 领域服务（乐观锁） | - |
| Phase 2 | 认证层（三类身份）+ Redis 缓存 + 中间件 | **M1 骨架可用** |
| Phase 3 | 计费 + 模型路由 + 端到端编排 | **M2 核心闭环** |
| Phase 4 | 56 个管理接口 | - |
| Phase 5 | 审计 + 限流 + 80% 告警 | **M3 功能完整** |
| Phase 6 | 集成测试 + Docker 打磨 + 文档 | **M4 可部署** |

---

## 12. 关键技术决策（固化）

1. **计费权威**：上游 `usage` 为准；tiktoken 仅用于预冻结估算 + 无 usage 兜底（标记 fallback）。
2. **并发安全**：所有扣减走 `UPDATE … WHERE version=? RETURNING`（乐观锁），不用悲观锁。
3. **预冻结→结算**：`credit_holds` 表，调用前冻结估算额，调用后按实际 usage 多退少补。
4. **积分模型**：整数运算（`i64`），汇率"每 1000 token = N 积分"，admin 可调。
5. **多维度统计**：`usage_logs` 双标签 + 预聚合表 UPSERT，事务内同步更新。
6. **模型路由**：provider 标 level，strategy 可配，4xx 欠费→disabled（不自动恢复）+ 降级；80% 告警（防重复）。
7. **认证**：Admin=密码+TOTP+有状态 token；Service=client_credentials→JWT；API Token=静态 Bearer+HMAC hash。
8. **Redis**：独立服务，仅缓存，权威在 SQLite。
9. **双端口**：单容器单进程双 listener :8080/:8081。
10. **tun 接入**：frpc 独立容器，只暴露 :8081，`<random>.tun.alius.tech`，Caddy 终结 TLS。
11. **双模式（新增）**：单一二进制，`RUN_MODE=server|agent` 决定角色。Server=云端/容器核心服务；Agent=桌面迷你 Server + FRP 内嵌 + GUI。核心代码（storage/auth/billing/router）两种模式完全复用。
12. **设备身份（新增）**：第四类身份 Device Agent（device_id/device_key），仅作 Console 管理 Agent 的接入凭证。

---

## 13. 双模式架构（Server / Agent）

### 13.1 设计原则

**Agent 就是一个"带 FRP 远程接入 + 桌面 GUI 的迷你 Server"。**

- **单一二进制**：`token_hub` 一个可执行文件，通过 `RUN_MODE` 环境变量或 `--mode` 参数决定角色。
- **核心代码完全复用**：storage / auth / billing / router-llm / api 五个 crate 在两种模式下行为一致。Agent 不是"瘦客户端"，它包含完整的 Server 能力。
- **Server 与 Agent 完全独立**：Agent 有自己的本地 SQLite，本地账号体系独立，不依赖云端 Server（MVP 阶段无心跳/注册）。

### 13.2 两种模式对比

| 维度 | Server 模式 | Agent 模式 |
|---|---|---|
| 部署 | Docker / 云服务器 | Windows/macOS 桌面 |
| 数据库 | SQLite（容器卷） | SQLite（用户目录，本地账号独立） |
| Chat API (:8080) | 绑 0.0.0.0 或经 tun 暴露 | 仅绑 127.0.0.1（本地应用调用） |
| Admin API (:8081) | 经 tun 远程接入 | 经 FRP 隧道远程接入 |
| FRP | 可选（独立 frpc 容器） | 内嵌（进程内 spawn frpc 子进程） |
| GUI | 无（守护进程） | 系统托盘 + 配置窗口 |
| Redis | 独立服务 | 本地实例或内存降级 |
| 身份 | Admin / Service / API User | 同 + Device Agent（Console 接入凭证） |

### 13.3 Agent 模式数据流

```
本地应用 (localhost:8080)
    │ /v1/chat/completions（本地 API Token 鉴权）
    ▼
┌──────────────────────────────────────┐
│  TokenHub Agent（桌面常驻）            │
│  ├─ 本地 HTTP Server (:127.0.0.1)     │
│  │   ├─ Chat API → 本地模型路由 → 上游  │
│  │   └─ Admin API → 本地 SQLite         │
│  ├─ 本地 SQLite（本地账号/Token/积分）  │
│  ├─ FRP 客户端（内嵌子进程）            │
│  │   └─ 隧道: <random>.tun.alius.tech   │
│  └─ 系统托盘 GUI（状态/配置/退出）      │
└──────────────────────────────────────┘
                ▲
                │ FRP 隧道（HTTPS via Caddy）
                │
        Console（手机/平板）
        管理员用 device_key 或 TOTP 登录
        操作的是 Agent 本地数据
```

**关键：Console 经 FRP 直达 Agent，操作 Agent 本地数据。** 不转发到云端 Server。

### 13.4 设备代理身份（Device Agent）

第四类身份，简化实现：

- **用途**：Console（管理员手机端）访问 Agent 时的接入凭证。Agent 初始化时生成 `device_id` + `device_key`（明文仅显示一次），管理员在 Console 录入后即可管理该 Agent。
- **不用于**：Chat 调用（Chat 仍用 API Token）、Agent↔Server 通信（两者独立）。
- **存储**：Agent 本地 `device_credentials` 表，存 `device_key_hash`（HMAC），明文不存。
- **认证流程**：Console 带 `device_id` + `device_key` 请求 Agent 的 admin 接口 → Agent 校验 HMAC → 颁发本地 session → 后续操作。
- **安全**：device_key 可重置（生成新密钥，旧密钥失效）；device 仅能管理本 Agent 本地资源。

### 13.5 内嵌 FRP 客户端（Agent 模式）

Agent 不再依赖外部 frpc 容器，而是在进程内管理 frpc：

1. **配置生成**：从 Agent 配置（frp.server / frp.token / frp.subdomain）生成临时 `frpc.toml`。
2. **子进程启动**：spawn `frpc -c <toml>` 子进程（frpc 二进制随 Agent 打包分发）。
3. **监控重连**：监控子进程存活，崩溃自动重启；记录隧道 URL。
4. **对外不可见**：对外文档称"远程管理接入"，实现细节（frp/frpc）不暴露。

### 13.6 桌面 GUI（Agent 模式）

- **系统托盘**（tray-icon，Tauri 出品）：跨平台菜单栏图标 + 右键菜单。
  - 菜单：在线状态、FRP 隧道地址、打开配置、重启服务、退出。
  - macOS：`LSUIElement=true`（不显示 Dock 图标，纯菜单栏）。
- **配置窗口**（tao + egui）：首次启动引导 + 日常配置。
  - 字段：本地监听端口、FRP 服务地址/Token/子域名、设备名称。
  - 首次启动：填写 FRP 配置 → 生成 device 凭证（展示 device_id/device_key 一次）→ 启动。
- **开机自启**：macOS 用 LaunchAgent；Windows 用注册表 Run 键或计划任务。

### 13.7 配置扩展

`.env` / 配置文件新增字段：

```
# 运行模式：server | agent（默认 server）
RUN_MODE=server

# === Agent 模式专用 ===
AGENT_NAME=my-laptop          # 设备显示名
AGENT_PLATFORM=macos          # windows/macos/linux

# FRP（Agent 内嵌 frpc 用）
FRP_SERVER=frp.alius.tech
FRP_PORT=7000
FRP_TOKEN=                    # frps 鉴权 token（可空）
FRP_SUBDOMAIN=change-me       # <random>.tun.alius.tech

# 设备凭证（首次生成后回填，后续自动读取）
DEVICE_ID=
DEVICE_KEY=
```

### 13.8 docker-compose 调整说明

- **Server 模式**：保持现有三服务（tokenhub + tun + redis）。tun 作为可选的远程管理接入。
- **Agent 模式**：**不使用 Docker**。桌面环境用原生安装包（macOS .app / Windows .exe），内含 token_hub 二进制 + frpc 二进制。

### 13.9 打包分发

| 平台 | 产物 | 内容 |
|---|---|---|
| Docker | 镜像 | Server 模式，含 token_hub + 迁移 |
| macOS | `.app` bundle | Agent 模式，token_hub + frpc + 开机自启配置 |
| Windows | `.exe` + 安装器 | Agent 模式，token_hub + frpc.exe + 注册表自启 |

### 13.10 新增数据模型

```sql
-- 设备凭证（Agent 本地，仅 Agent 模式使用）
CREATE TABLE IF NOT EXISTS device_credentials (
  device_id TEXT PRIMARY KEY,
  device_key_hash TEXT NOT NULL,   -- HMAC-SHA256 hash
  name TEXT NOT NULL,
  platform TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER
);
```

