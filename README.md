# rs-admin-glm

[![backend](https://github.com/nasa-du/rs-admin-glm/actions/workflows/backend.yml/badge.svg?branch=main)](https://github.com/nasa-du/rs-admin-glm/actions/workflows/backend.yml)
[![frontend](https://github.com/nasa-du/rs-admin-glm/actions/workflows/frontend.yml/badge.svg?branch=main)](https://github.com/nasa-du/rs-admin-glm/actions/workflows/frontend.yml)

rs-admin 管理后台 monorepo：`frontend/`（shadcn-admin v2.2.1 fork 裁剪，TanStack Router + shadcn/ui）
+ `backend/`（Rust Axum 单 crate，sqlx + PostgreSQL 18）。登录 / 刷新轮换 / 三档 RBAC /
用户管理 / 个人设置均已上线（[#15]–[#19]）；CI 双门禁（backend / frontend workflow，paths
过滤互不触发）见 [#20]；部署基线（多阶段镜像 + 同源伺服）见 [#21]。

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

- 环境变量说明见 `backend/.env.example`（对应 spec §9.2）。

## 生产部署（同源）

多阶段镜像（根 `Dockerfile`：node 构建前端 → cargo 构建后端 → debian-slim 运行镜像）。运行镜像内
axum 以静态目录伺服前端构建产物并带 SPA 回退——页面与 `/api` 同端口同源，无 CORS（ADR 0001）。
`APP_ENV=production`：JSON 日志、Swagger 不挂载（`/swagger-ui` 404）。

```bash
# 1. 生成密钥并一键起整套系统（db + backend；密钥走 env 注入，不进镜像不进仓库）
JWT_SECRET="$(openssl rand -base64 48)" POSTGRES_PASSWORD="$(openssl rand -hex 24)"   docker compose -f docker-compose.prod.yml up -d --build

# 2. 冷启动首位超级管理员（spec §2.3；密码随机生成、仅打印一次）
docker compose -f docker-compose.prod.yml exec backend   rs-admin-backend seed-superadmin --email admin@example.com

# 3. 浏览器打开 http://localhost:3000 —— 登录 → 建用户 → 拿初始密码 → 首登改密 → 使用
```

- 前端路由（如 `/users`）硬刷新由 SPA 回退直接回 `index.html`；未匹配的 `/api` 路径仍答 JSON 错误体。
- 数据库不暴露宿主端口，数据落 `pgdata` 卷；两服务挂 `unless-stopped`。
- 环境变量清单见 `backend/.env.example`（spec §9.2）；K8s / TLS 终止 / 备份 / 监控不在基线内（spec §9.3）。

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
Dockerfile + docker-compose.prod.yml   # 部署基线（多阶段镜像 + 同源伺服，spec §9.3）
```

[#13]: https://github.com/nasa-du/rs-admin-glm/issues/13
[#14]: https://github.com/nasa-du/rs-admin-glm/issues/14
[#15]: https://github.com/nasa-du/rs-admin-glm/pull/15
[#19]: https://github.com/nasa-du/rs-admin-glm/pull/19
[#20]: https://github.com/nasa-du/rs-admin-glm/issues/20
[#21]: https://github.com/nasa-du/rs-admin-glm/issues/21
