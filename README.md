# rs-admin-glm

[![backend](https://github.com/nasa-du/rs-admin-glm/actions/workflows/backend.yml/badge.svg?branch=main)](https://github.com/nasa-du/rs-admin-glm/actions/workflows/backend.yml)
[![frontend](https://github.com/nasa-du/rs-admin-glm/actions/workflows/frontend.yml/badge.svg?branch=main)](https://github.com/nasa-du/rs-admin-glm/actions/workflows/frontend.yml)

rs-admin 管理后台 monorepo：`frontend/`（shadcn-admin v2.2.1 fork 裁剪，TanStack Router + shadcn/ui）
+ `backend/`（Rust Axum 单 crate，sqlx + PostgreSQL 18）。登录 / 刷新轮换 / 三档 RBAC /
用户管理 / 个人设置均已上线（[#15]–[#19]）；CI 双门禁（backend / frontend workflow，paths
过滤互不触发）见 [#20]。

完整蓝图见 [docs/spec.md](docs/spec.md)，词汇表见 [CONTEXT.md](CONTEXT.md)，决策记录见 [docs/adr/](docs/adr/)。

## 开发环境

前置：Rust ≥ 1.94（仓库内 `backend/rust-toolchain.toml` 自动钉版）、Node 22+、Docker。

```bash
# 1. 数据库（postgres:18-alpine，仅此一个服务）
docker compose up -d db

# 2. 后端（127.0.0.1:3000；启动即跑迁移，幂等）
cd backend
cp .env.example .env
cargo run

# 3. 冷启动首位超级管理员（spec §2.3；密码随机生成、仅打印一次）
cargo run -- seed-superadmin --email <email>   # [--password <password>] 可选

# 4. 前端（http://localhost:5173，/api 一条规则代理到 127.0.0.1:3000）
cd ../frontend
npm install
npm run dev
```

- mock 登录：任意邮箱 + 密码即可进入 dashboard（真实认证在 M2/M4 接入）。
- 环境变量说明见 `backend/.env.example`（对应 spec §9.2）。

## 门禁

本地与 CI 同一套门禁（CI 按 `backend/**` / `frontend/**` 路径分别触发各自 workflow，根级变更两者都跑）：

| 范围 | 命令 |
|---|---|
| 后端 | `cd backend && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`（测试需 `docker compose up -d db`，每测试一座临时库；CI 另跑 `sqlx migrate run` + `prepare --check`） |
| 前端 | `cd frontend && npm run lint && npm run build && npm run locales:check`（= eslint + tsc -b + vite build + 双语 locale key 对齐） |

## 目录

```
frontend/   # shadcn-admin v2.2.1 fork：dashboard / users / sign-in / settings + 403/500
backend/    # Axum：tracing + PgPool + 启动迁移；migrations（users/refresh_tokens）+ seed CLI + AppError 骨架
docs/       # spec、ADR
```

[#13]: https://github.com/nasa-du/rs-admin-glm/issues/13
[#14]: https://github.com/nasa-du/rs-admin-glm/issues/14
[#15]: https://github.com/nasa-du/rs-admin-glm/pull/15
[#19]: https://github.com/nasa-du/rs-admin-glm/pull/19
[#20]: https://github.com/nasa-du/rs-admin-glm/issues/20
