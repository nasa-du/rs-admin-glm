-- refresh_tokens 表（docs/spec.md §2.2，决议 #3 / ADR 0001）：
-- 一次登录 = 一条令牌族；DB 只存 SHA-256 hex，明文不落库；
-- consumed_at 非空 = 已被轮换，再次出现即重用 → 整族撤销；
-- 7 天滑动过期（每次轮换重置 expires_at）；删用户时族随外键级联消失。
CREATE TABLE refresh_tokens (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id   UUID NOT NULL,                       -- 一次登录 = 一条令牌族
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash  CHAR(64) NOT NULL UNIQUE,            -- refresh 明文的 SHA-256 hex；明文不落库
  expires_at  TIMESTAMPTZ NOT NULL,                -- 每次轮换重置（7 天滑动）
  consumed_at TIMESTAMPTZ,                         -- 非空 = 已被轮换；再次出现 = 重用 → 整族撤销
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_refresh_tokens_family ON refresh_tokens (family_id);
CREATE INDEX idx_refresh_tokens_user   ON refresh_tokens (user_id);
