-- TokenHub v1 初始 schema
-- 主键统一 TEXT；时间戳 INTEGER(Unix ms)；外键需开启 PRAGMA foreign_keys。

-- 管理员
CREATE TABLE IF NOT EXISTS admin_users (
  id TEXT PRIMARY KEY,
  phone TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  totp_secret TEXT NOT NULL,
  roles TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  last_login_at INTEGER
);

-- 管理员有状态 access token（可即时吊销）
CREATE TABLE IF NOT EXISTS admin_access_tokens (
  token_hash TEXT PRIMARY KEY,
  admin_id TEXT NOT NULL REFERENCES admin_users(id),
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_admin_tokens_admin ON admin_access_tokens(admin_id);

-- 管理员 refresh token
CREATE TABLE IF NOT EXISTS admin_refresh_tokens (
  token_hash TEXT PRIMARY KEY,
  admin_id TEXT NOT NULL REFERENCES admin_users(id),
  access_token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_admin ON admin_refresh_tokens(admin_id);

-- 策略模板（accounts 引用，需先建）
CREATE TABLE IF NOT EXISTS policies (
  id TEXT PRIMARY KEY,
  name TEXT UNIQUE NOT NULL,
  allowed_models TEXT NOT NULL DEFAULT '[]',
  monthly_credit_cap INTEGER,
  description TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 账号
CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY,
  external_id TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  policy_id TEXT REFERENCES policies(id),
  note TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_accounts_external ON accounts(external_id);
CREATE INDEX IF NOT EXISTS idx_accounts_status ON accounts(status);

-- API Token（DB 存 HMAC hash）
CREATE TABLE IF NOT EXISTS api_tokens (
  id TEXT PRIMARY KEY,
  token_hash TEXT UNIQUE NOT NULL,
  prefix TEXT NOT NULL,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  name TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  expires_at INTEGER,
  created_at INTEGER NOT NULL,
  revoked_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_tokens_account ON api_tokens(account_id);

-- Service 账号
CREATE TABLE IF NOT EXISTS service_accounts (
  id TEXT PRIMARY KEY,
  client_id TEXT UNIQUE NOT NULL,
  client_secret_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  scopes TEXT NOT NULL DEFAULT '[]',
  ip_whitelist TEXT,
  public_key TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 逻辑模型
CREATE TABLE IF NOT EXISTS models (
  id TEXT PRIMARY KEY,
  logical_name TEXT UNIQUE NOT NULL,
  description TEXT,
  input_rate_per_1k INTEGER NOT NULL,
  output_rate_per_1k INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 供应商凭证
CREATE TABLE IF NOT EXISTS provider_credentials (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  provider_type TEXT NOT NULL,
  base_url TEXT NOT NULL,
  api_key_enc TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  disabled_reason TEXT,
  disabled_at INTEGER,
  quota_limit INTEGER,
  quota_used INTEGER NOT NULL DEFAULT 0,
  quota_threshold INTEGER NOT NULL DEFAULT 80,
  quota_alert_sent INTEGER NOT NULL DEFAULT 0,
  quota_synced_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 逻辑模型 ↔ 供应商映射
CREATE TABLE IF NOT EXISTS model_providers (
  id TEXT PRIMARY KEY,
  logical_model_id TEXT NOT NULL REFERENCES models(id),
  provider_id TEXT NOT NULL REFERENCES provider_credentials(id),
  upstream_model TEXT NOT NULL,
  level INTEGER NOT NULL,
  weight INTEGER NOT NULL DEFAULT 100,
  strategy TEXT NOT NULL DEFAULT 'sequential',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_model_providers_model ON model_providers(logical_model_id);
CREATE INDEX IF NOT EXISTS idx_model_providers_provider ON model_providers(provider_id);

-- 上游模型（厂商真实模型，待标注）
CREATE TABLE IF NOT EXISTS upstream_models (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL REFERENCES provider_credentials(id),
  upstream_model TEXT NOT NULL,
  level INTEGER,
  labeled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_upstream_models_provider ON upstream_models(provider_id);

-- 积分余额（乐观锁）
CREATE TABLE IF NOT EXISTS credits (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id),
  balance INTEGER NOT NULL DEFAULT 0,
  held INTEGER NOT NULL DEFAULT 0,
  version INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);

-- 积分流水
CREATE TABLE IF NOT EXISTS credit_transactions (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  delta INTEGER NOT NULL,
  balance_after INTEGER NOT NULL,
  reason TEXT,
  operator TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_credit_tx_account ON credit_transactions(account_id, created_at);

-- 预冻结（hold）
CREATE TABLE IF NOT EXISTS credit_holds (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  amount INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'held',
  request_id TEXT,
  created_at INTEGER NOT NULL,
  settled_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_holds_account ON credit_holds(account_id, status);

-- 调用日志（事实表）
CREATE TABLE IF NOT EXISTS usage_logs (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  logical_model TEXT NOT NULL,
  provider_id TEXT,
  upstream_model TEXT,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  total_tokens INTEGER,
  credits_cost INTEGER NOT NULL,
  usage_source TEXT NOT NULL,
  status TEXT NOT NULL,
  source_ip TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_usage_account_time ON usage_logs(account_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_provider_time ON usage_logs(provider_id, created_at);

-- 用户用量预聚合
CREATE TABLE IF NOT EXISTS account_usage_summary (
  account_id TEXT NOT NULL,
  period TEXT NOT NULL,
  prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  credits INTEGER NOT NULL DEFAULT 0,
  calls INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, period)
);

-- 供应商用量预聚合
CREATE TABLE IF NOT EXISTS provider_usage_summary (
  provider_id TEXT NOT NULL,
  period TEXT NOT NULL,
  tokens_used INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (provider_id, period)
);

-- 操作审计日志
CREATE TABLE IF NOT EXISTS audit_logs (
  id TEXT PRIMARY KEY,
  actor_kind TEXT NOT NULL,
  actor_id TEXT,
  action TEXT NOT NULL,
  target_type TEXT,
  target_id TEXT,
  detail TEXT,
  source_ip TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_logs(actor_kind, actor_id);
