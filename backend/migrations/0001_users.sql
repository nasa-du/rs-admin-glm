-- users 表（docs/spec.md §2.1，决议 #6）：
-- uuid 主键 = 将来的 JWT sub；email 唯一登录标识（格式校验在应用层）；
-- 单值层级角色；硬删除、无软删、无 accountNo。
CREATE TYPE user_role AS ENUM ('superadmin', 'admin', 'user');

CREATE TABLE users (
  id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),  -- = JWT sub
  email                TEXT NOT NULL UNIQUE,       -- 登录标识；格式校验在应用层
  display_name         TEXT NOT NULL,              -- 显示名，1–32 字符（按字符计，应用层校验）
  password_hash        TEXT NOT NULL,              -- argon2id PHC 字符串
  role                 user_role NOT NULL DEFAULT 'user',  -- 单值层级角色
  is_enabled           BOOLEAN NOT NULL DEFAULT true,
  must_change_password BOOLEAN NOT NULL DEFAULT false,
  created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
