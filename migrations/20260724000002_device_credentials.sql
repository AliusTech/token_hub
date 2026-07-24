-- 设备凭证表（Agent 模式专用）
-- 用于 Console（管理员手机端）访问 Agent 时的接入凭证

CREATE TABLE IF NOT EXISTS device_credentials (
  device_id TEXT PRIMARY KEY,
  device_key_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  platform TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER
);
