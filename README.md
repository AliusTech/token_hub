# TokenHub

> 企业级 LLM 应用基础设施服务（AI Gateway）：统一 OpenAI 兼容接口 + 多供应商聚合 + 积分计费 + 多租户治理。

[![Rust](https://img.shields.io/badge/Rust-1.96-orange)](https://www.rust-lang.org/)

## 功能特性

- **统一 API**：OpenAI 兼容的 `/v1/chat/completions`，下游无需感知后端模型
- **多模型池化**：供应商抽象为逻辑模型（basic/standard/expert），分级路由 + 负载均衡 + 自动降级
- **积分计费**：整数汇率体系，预冻结→结算，乐观锁保证并发不超卖
- **计量权威**：以上游 `usage` 为准，tiktoken 仅作预冻结估算 + 无 usage 兜底
- **四类身份**：管理员(TOTP) / 服务系统(JWT) / 应用用户(API Token) / 设备代理(Device)
- **成本监控**：供应商额度 80% 阈值告警 + 配额耗尽自动切换
- **双模式**：Server（云端/Docker）+ Agent（桌面，带远程管理接入）

## 架构

```
              ┌──────────────┐  ┌──────────────┐
  应用侧 ────►│ Chat API     │  │ Admin API    │◄──── 管理员(Console)
              │ :8080        │  │ :8081        │      经远程管理接入(tun)
              └──────┬───────┘  └──────┬───────┘
                     │                 │
              ┌──────┴─────────────────┴──────┐
              │       TokenHub 核心            │
              │  鉴权→预冻结→路由→结算→记日志   │
              ├───────────────────────────────┤
              │  SQLite (权威) │ 缓存 (内存/Redis)│
              └───────────────────────────────┘
                     │
              ┌──────▼──────────────┐
              │  上游 LLM 供应商      │
              │  OpenAI/Claude/...  │
              └─────────────────────┘
```

### 双模式

| 维度 | Server 模式 | Agent 模式 |
|---|---|---|
| 部署 | Docker / 云服务器 | Windows/macOS 桌面 |
| 数据库 | SQLite（容器卷） | SQLite（本地，独立账号） |
| 监听 | 0.0.0.0（公网/内网） | 127.0.0.1（仅本地） |
| 远程接入 | 可选 tun 容器 | 内嵌 FRP 客户端 |
| 缓存 | 内存（默认）/ Redis | 内存（默认） |

同一二进制，通过 `RUN_MODE=server|agent` 切换。

## 快速启动

### Docker（Server 模式）

```bash
# 1. 复制配置
cp .env.example .env
# 编辑 .env：设置 SERVER_SECRET、TUN_SUBDOMAIN

# 2. 启动
docker compose up -d

# 3. 初始化首个管理员
docker compose exec tokenhub token_hub admin create \
  --database-url "sqlite:///data/tokenhub.db" \
  --server-secret "$SERVER_SECRET" \
  --phone "13800138000" --password "your-password"

# 4. 验证
curl http://localhost:8080/healthz
curl http://localhost:8081/healthz
```

### 本地开发

```bash
# 构建
cargo build

# 运行（Server 模式，内存缓存）
DATABASE_URL="sqlite:///data/tokenhub.db" \
REDIS_URL="" \
SERVER_SECRET="dev-secret" \
cargo run

# 另一终端：初始化管理员
DATABASE_URL="sqlite:///data/tokenhub.db" \
SERVER_SECRET="dev-secret" \
cargo run -- admin create --phone "13800138000" --password "pass"
```

### Agent 模式（桌面）

```bash
RUN_MODE=agent \
DATABASE_URL="sqlite://~/.tokenhub/agent.db" \
SERVER_SECRET="agent-secret" \
CHAT_LISTEN="127.0.0.1:8080" \
ADMIN_LISTEN="127.0.0.1:8081" \
FRP_SERVER="frp.alius.tech" \
FRP_SUBDOMAIN="my-device" \
./token_hub
```

## 配置项

通过环境变量配置（Docker 友好）：

| 变量 | 默认值 | 说明 |
|---|---|---|
| `RUN_MODE` | `server` | 运行模式：server / agent |
| `DATABASE_URL` | `sqlite:///data/tokenhub.db` | SQLite 路径 |
| `REDIS_URL` | `redis://redis:6379` | Redis 地址（空则用内存缓存） |
| `SERVER_SECRET` | - | HMAC 密钥（务必改为强随机） |
| `CHAT_LISTEN` | `0.0.0.0:8080` | Chat API 监听 |
| `ADMIN_LISTEN` | `0.0.0.0:8081` | Admin API 监听 |
| `FRP_SERVER` | - | 远程管理接入服务地址（Agent 模式） |
| `FRP_PORT` | `7000` | 接入服务端口 |
| `FRP_TOKEN` | - | 接入服务鉴权 token |
| `FRP_SUBDOMAIN` | - | 子域名前缀（`<sub>.tun.alius.tech`） |

## CLI 命令

```bash
# 初始化管理员
token_hub admin create --phone <phone> --password <pwd> [--role super_admin]

# 环境变量自动读取：DATABASE_URL / SERVER_SECRET
```

## 缓存策略

- **默认内存缓存**（DashMap + TTL），零依赖，适合单机/Agent
- **可选 Redis**：配置 `REDIS_URL` 自动切换，适合高并发/分布式
- 统一 `CacheBackend` trait，业务层无感切换
- 权威数据永远在 SQLite，缓存仅加速热路径

## API 接口

- Chat API（:8080）：`/v1/chat/completions`、`/v1/models`、`/v1/usage`
- Admin API（:8081）：56 个管理接口（认证/账号/Token/积分/模型/供应商/映射/Service/策略/报表/审计/设备）

详见 [设计文档](docs/specs/2026-07-24-tokenhub-backend-design.md)。

## 测试

```bash
cargo test --workspace
```

核心测试覆盖：
- 乐观锁并发扣费不超卖（10 线程 / 20 线程压测）
- 汇率计算整数精度
- TOTP / JWT / API Token HMAC 认证
- 模型分级路由 + 自动降级
- usage 权威 + tiktoken 兜底
- 内存缓存 TTL 过期

## 技术栈

| 层 | 技术 |
|---|---|
| 语言 | Rust 2021 |
| Web | axum + tower |
| 数据库 | sqlx (SQLite)，WAL + 乐观锁 |
| 缓存 | DashMap (内存) / Redis (可选) |
| 分词 | tiktoken-rs |
| 桌面 GUI | tray-icon (Tauri) — 规划中 |
| 部署 | Docker / 原生二进制 |

## 项目结构

```
token_hub/
├── crates/
│   ├── domain/         # 领域模型
│   ├── storage/        # SQLite Repo + 迁移
│   ├── cache/          # 可插拔缓存（内存/Redis）
│   ├── auth/           # TOTP/JWT/API Token/Session
│   ├── billing/        # 汇率/预冻结/结算/tiktoken
│   ├── router-llm/     # 分级路由/上游代理/降级/熔断
│   ├── audit/          # mpsc 批量写 + 告警 Notifier
│   ├── api/            # axum 双 router + handler + 中间件
│   ├── agent/          # Agent 模式（FRP + runtime）
│   └── cli/            # admin create
├── src/main.rs         # 入口（Server/Agent 分流）
├── migrations/         # SQL 迁移
└── docs/specs/         # 设计文档
```

## 许可证

私有项目。
