# rs-admin 登录系统实施方案（spec）

> 本 spec 是 wayfinder 地图 [#1](https://github.com/nasa-du/rs-admin-glm/issues/1) 的交付物（[实施方案汇总定稿 #9](https://github.com/nasa-du/rs-admin-glm/issues/9)），汇编八项已关闭决议：RBAC 矩阵（#2）、JWT 方案（#3）、界面双语（#4）、后端选型（#5）、用户模型（#6）、API 契约（#7）、生态调研（#8）、测试与 CI（#11）。
> **决策依据与被拒选项见四份 ADR（`docs/adr/0001`–`0004`），术语以根目录 `CONTEXT.md` 词汇表为准。实现会话以本文档为唯一施工蓝图，读后无需再问即可开工。**

## 0. 总览

fork [satnaing/shadcn-admin](https://github.com/satnaing/shadcn-admin) v2.2.1 裁剪为 dashboard / users / sign-in / settings 四组页面，接入 zh-CN / en 双语；后端 Rust Axum 单 crate（sqlx + jsonwebtoken + argon2）+ PostgreSQL 18；superadmin ⊃ admin ⊃ user 三档层级 RBAC；用户管理 CRUD；认证为 access JWT（15 min，前端仅内存）+ 不透明 refresh（7 天滑动，RFC 9700 轮换 + 重用检测 + 令牌族撤销，HttpOnly cookie）。

仓库形态：monorepo——`frontend/` + `backend/`。

### 0.1 技术栈钉版

| 用途 | 选择 | 备注 |
|---|---|---|
| Web 框架 | axum 0.8.9 | 路径语法 `{id}`；extractor 原生 async |
| 数据层 | sqlx 0.9.0 | `query_as!` 编译期校验；`cargo sqlx prepare` 产 `.sqlx/` 离线缓存（提交）；`sqlx::migrate!` 启动迁移。features: `runtime-tokio` + `tls-rustls` + `chrono` + `uuid` |
| JWT | jsonwebtoken 11.1.0 | HS256；feature `rust_crypto`；keys 启动构造一次复用；默认 Validation（60s leeway、验 exp） |
| 密码哈希 | argon2 0.6.0 | `Argon2::default()` 即 OWASP 基线（Argon2id, m=19456 KiB / t=2 / p=1） |
| 中间件 | tower-http 0.7.1 | Trace + Timeout + Compression；**CORS 不配**（生产同源 + 开发 Vite proxy，浏览器视角永远同源，见 ADR 0001） |
| OpenAPI | utoipa 6.0.0 + utoipa-axum 0.3 + swagger-ui 10.0.2 | `OpenApiRouter` + `routes!` 写法；**Swagger UI 仅开发环境挂载** |
| 观测 | tracing + tracing-subscriber | EnvFilter；dev pretty / prod JSON；不上 Sentry 类（v1 不做） |
| 运行时 | tokio（feature `full`） | — |
| 工具链 | Rust ≥ 1.94 | `rust-toolchain.toml` 提交仓库；sqlx 0.9 要求 |
| 前端 | shadcn-admin v2.2.1 fork | TanStack Router + TanStack Query + Zustand + Tailwind + shadcn/ui + react-hook-form + zod |
| i18n | i18next + react-i18next + i18next-browser-languagedetector | 版本在实现期核实钉版（ADR 0002） |
| 数据库 | PostgreSQL 18 | `postgres:18-alpine`，与 CI 对齐（ADR 0004） |

### 0.2 实现期防坑清单（来自 #8 调研，均已核实）

- sqlx 0.9 动态拼接 SQL 须包 `AssertSqlSafe`。
- sqlx-cli 安装**不带 `--locked`**（0.9 仓库无 Cargo.lock）：`cargo install sqlx-cli --version 0.9.0 --no-default-features --features postgres,rustls`。**勘误（PR #22）**：原命令漏 `postgres`，`--no-default-features` 下装出无驱动的 CLI，详见 ADR 0004 勘误。
- argon2 0.6 API：`SaltString::generate()` 不再收 RNG 参数，`hash_password` 自动加盐——勿照抄 0.5 旧教程。
- 同一 binary 的 TLS crypto provider 保持全局一致（sqlx 的 rustls 与其它 crate 的 ring / aws-lc-rs 不混用）。
- 参考样板：`nakamuraos/axum-postgres-boilerplate`（axum 0.8.9 + JWT + RBAC + utoipa，与本栈同构）；官方 `sqlx-postgres`、`tracing-aka-logging` 示例。**勿用** `realworld-axum-sqlx`（停更于 axum 0.3）与 `zero-to-production`（actix-web）。
- sqlx 仓库已迁 transact-rs org、0.9.0 后无 stable 补丁——**锁版本用，不动辄升级**（风险已接受，#5）。

## 1. 目录结构

```
rs-admin-glm/
├── frontend/                      # shadcn-admin v2.2.1 fork（裁剪）
│   ├── src/
│   │   ├── main.tsx               # i18n 初始化；queryCache 删 401 分支
│   │   ├── routes/                # TanStack Router 文件路由
│   │   │   ├── _authenticated/route.tsx   # beforeLoad 三层守卫（§6.2）
│   │   │   ├── _authenticated/dashboard/
│   │   │   ├── _authenticated/users/      # 列表页 + 四个 dialog
│   │   │   ├── _authenticated/settings/   # profile / password / appearance
│   │   │   └── (auth)/sign-in/
│   │   ├── components/            # 模板组件，裁剪后保留
│   │   ├── features/users/        # users 页数据 hook 与类型
│   │   ├── store/auth.ts          # useAuthStore 重写（仅内存）
│   │   ├── lib/axios.ts           # 新建：拦截器 + 单飞 refresh（§6.2）
│   │   ├── i18n.ts                # i18next 装配
│   │   └── locales/{zh-CN,en}.json
│   └── scripts/check-locales.mjs  # locale key 对齐脚本（零依赖 node）
└── backend/
    ├── Cargo.toml                 # 单 crate，不开 workspace
    ├── rust-toolchain.toml        # ≥1.94
    ├── .sqlx/                     # 离线缓存，提交（CI 用）
    ├── migrations/
    │   ├── 0001_users.sql         # user_role 枚举 + users 表
    │   └── 0002_refresh_tokens.sql
    └── src/
        ├── main.rs                # CLI 子命令解析、sqlx::migrate!、路由组装、listener、Swagger dev 挂载
        ├── state.rs               # AppState { PgPool, JWT keys, config }；extractor 取池用 PgPool::from_ref
        ├── error.rs               # AppError → 错误码 + HTTP status（承载语义码边界，ADR 0002）
        ├── auth/                  # login / refresh / logout / password + AuthUser extractor + 轮换与族撤销
        ├── users/                 # CRUD + password-reset + /users/me + 列表查询 + RBAC 与护栏
        ├── seed.rs                # seed-superadmin 子命令
        └── tests/                 # 集成测试（#[sqlx::test]，五类，见 §7）
```

根目录新增：`docker-compose.yml`（dev 起 PG，见 §9）、`.github/workflows/{backend,frontend}.yml`（§7）。单测以 `#[cfg(test)]` 内联在模块里，集成测试在 `tests/`。

## 2. 数据模型

### 2.1 `migrations/0001_users.sql`

```sql
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
```

**说明**（决议 #6）：无 accountNo（uuid + email + displayName 已够）；email 格式应用层校验 + DB 唯一兜底；密码 8–128、无组成规则（长度优先，上限防 argon2 超长 DoS）；**硬删除**、无软删。

### 2.2 `migrations/0002_refresh_tokens.sql`

```sql
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
```

**语义**（决议 #3 / ADR 0001）：

- 撤销一条族 = `DELETE … WHERE family_id = $1`；撤销某用户全部族 = `DELETE … WHERE user_id = $1`；删除用户时族随外键级联消失（= 全族撤销）。
- 轮换 = 旧行置 `consumed_at`、插入新行（新 `token_hash`、同 `family_id`、`expires_at = now() + 7d`）。
- 重用检测 = 呈上的哈希命中 `consumed_at IS NOT NULL` 的行 → 撤销整族、401。
- 该「哈希存储 + 轮换 + 重用检测」组合无权威 Rust 范文（#8 确认），属自实现工作量：一张表 + refresh 端点 + 既有 JWT 中间件。

### 2.3 首个超管：seed CLI 子命令

```
cargo run -- seed-superadmin --email <email> [--password <password>]
```

- `--password` 缺省时**随机生成并打印一次**（凭证当场取走；邮件 out of scope）。
- email 已存在则报错退出（不做更新）。
- 被拒方案与理由见 #6（迁移内置 INSERT、首启自动引导均否）。

### 2.4 初始密码

用户被创建或被管理员重置密码时：随机生成（16 字符、去除易混淆字符集）、**仅向操作者展示一次**、`must_change_password = true`；该用户下次登录被强制改密，初始密码随即作废（词汇表「初始密码」）。

## 3. RBAC 矩阵

层级单值角色：**superadmin ⊃ admin ⊃ user**，每人恰好一个角色，高档隐含低档全部权限（决议 #2）。档位数值：superadmin=3，admin=2，user=1。

| 能力 | user | admin | superadmin |
|---|---|---|---|
| dashboard | ✅ | ✅ | ✅ |
| settings（本人资料/密码 + 外观偏好） | ✅ | ✅ | ✅ |
| users 列表 | ❌ 不可见（403） | ✅ | ✅ |
| 创建 / 编辑 / 禁用 user | — | ✅ | ✅ |
| 删除任何用户 | — | ❌（仅可禁用） | ✅ |
| 创建 / 编辑 / 禁用 admin | — | ❌（可见不可动） | ✅ |
| 角色分配与变更（升降级） | — | — | ✅ 独占 |

**向下管理（Manage-down）**：任何角色只能创建、变更、禁用、删除角色等级**严格低于**自己的用户；对同级或更高角色至多可见。落在 API 上：目标用户的现有角色与新角色都必须严格低于操作者——**因此不可经 API 创建或晋升出超级管理员，第二名超管只能由 seed CLI 产生**（本 spec 固化 #7「body.role ≥ 自己等级 → 403」于 POST 与 PATCH 两端对称成立）。

**护栏（Guardrail）**，触碰即整体拒绝、不做部分生效：

1. 不能删除自己、不能禁用自己、不能变更自己的角色（`SELF_MANAGEMENT_FORBIDDEN`）；
2. 系统必须始终至少保留一名启用状态的超级管理员（`LAST_SUPERADMIN`，事务内校验）。

**执行点**：角色写入 JWT claims，后端 `AuthUser` extractor 验签后取 claims、路由级 `require_role` 分级校验，**不逐请求查库**；权限变更最迟在 access token 自然过期（≤15 min）后生效。前端：静态「角色→可见路由」映射表为单一事实源，路由 `beforeLoad` 守卫 + 侧边栏同表渲染（§6.2）。

## 4. JWT 与会话流程

（决议 #3，ADR 0001。）

### 4.1 参数

| 项 | 值 |
|---|---|
| access 形态 | HS256 JWT，TTL **15 min** |
| access claims | `sub`（用户 uuid）、`email`、`role`（单值枚举）、`iat`、`exp` |
| refresh 形态 | **256-bit 不透明随机串**（非 JWT），DB 只存 SHA-256 hex |
| refresh 有效期 | **7 天滑动**（每次轮换重置）；无绝对上限 |
| 令牌族 | 一次登录换发一条族；一个浏览器 = 一条族 = 一个会话 |
| cookie | 名 `rs_admin_refresh`；`HttpOnly; Secure; SameSite=Strict; Path=/api/auth` |
| CSRF | SameSite=Strict 主防线 + refresh / logout 两个吃 cookie 的端点要求自定义 header `X-Requested-With: XMLHttpRequest`（前端拦截器统一注入，不依赖 axios 默认行为） |
| 密钥 | `JWT_SECRET` env，≥32 字节随机；单密钥无 kid/JWKS；换密钥 = 全员下线（接受的灾难恢复语义） |
| 明确不做 | 全局黑名单、per-request 查库校验、kid/JWKS 密钥轮换 |

### 4.2 时序

- **登录** `POST /auth/login`：argon2 验密 → 签发 access + 生成 refresh（新族）→ `200 {accessToken, user}` + Set-Cookie。密码错 → 401 `INVALID_CREDENTIALS`（邮箱不存在与密码错同码，防枚举）；密码对但 `is_enabled = false` → 401 `USER_DISABLED`。`must_change_password` 用户照常登录，标志在 `user` 内带回、由前端守卫接管。
- **刷新** `POST /auth/refresh`（cookie + 自定义 header）：按哈希查行——
  - 不存在 → 401 `REFRESH_INVALID`；
  - 存在但 `consumed_at` 非空 → **重用检测**：撤销整族 → 401；
  - 存在但过期 → 401（顺手清理该族）；
  - 有效 → **重读 users 行**（防纵深）：用户已删/已禁用 → 撤销族 → 401（`USER_DISABLED` / `REFRESH_INVALID`）；角色已变 → 新 access 用**新角色**；然后轮换（旧行置 consumed、插新行、滑动 7 天）→ `200 {accessToken, user}` + 新 Set-Cookie。
  - 权限变更生效上界由此**结构性 ≤15 min**，不依赖各管理操作记得撤销。
- **登出** `POST /auth/logout`（cookie + header）：撤销当前族 + 清 cookie（Max-Age=0）→ `204`；无 cookie 也 204（幂等）。access 不拉黑，自然死亡。
- **改密** `PATCH /auth/password`：验当前密码（错 → 422 字段级，不用 401，避免触发拦截器无谓刷新重放）→ 写新哈希、`must_change_password = false`、**撤销该用户全部族（含当前）** → `204`；前端跳 sign-in 重登。首登强制改密同路。
- **禁用 / 删除用户 / 管理员重置密码**：撤销该用户**全部**族（禁用、重置显式撤销；删除靠外键级联）。

## 5. API 契约

（决议 #7，ADR 0003；错误码语义见 ADR 0002。）

### 5.1 骨架

- 所有端点挂 **`/api`** 前缀，不版本化；开发期 Vite proxy 一条规则 `/api → http://localhost:3000`，生产同源不配 CORS；axios `baseURL: '/api'`。
- JSON 一律 **camelCase**（后端 serde `rename_all = "camelCase"`）；时间 RFC 3339 字符串。
- `User` 形状：`{ id, email, displayName, role, isEnabled, mustChangePassword, createdAt, updatedAt }`；`role` 单值字符串枚举，前端 store 层包 `[role]` 适配模板数组形状（长度恒 1）。
- 鉴权：业务端点走 `Authorization: Bearer <access>`；refresh / logout 走 cookie + 自定义 header。

### 5.2 错误契约

```json
{ "code": "USER_DISABLED", "title": "This account has been disabled" }
```

`code` 稳定语义错误码（文案归前端 locale，后端永不感知界面语言）；`title` 英文兜底——模板现有 `error.response?.data.title` 消费点零改动可用；前端按 code + 界面语言映射，无映射回退 title。422 表单错误附字段级结构：

```json
{ "code": "VALIDATION_ERROR", "title": "Validation failed",
  "errors": { "email": ["EMAIL_TAKEN"], "currentPassword": ["CURRENT_PASSWORD_INCORRECT"] } }
```

**错误码清单**（初版；字段级码随实现可增，须同步进两份 locale 的 `errors` namespace）：

| code | HTTP | 触发 |
|---|---|---|
| `INVALID_CREDENTIALS` | 401 | 登录邮箱不存在或密码错误 |
| `USER_DISABLED` | 401 | 密码正确但被禁用；refresh 时用户已被禁用 |
| `EMAIL_TAKEN` | 409 | 创建用户邮箱重复（附 `errors.email`） |
| `VALIDATION_ERROR` | 422 | 字段校验失败（附 `errors` 字典：`EMAIL_INVALID` / `PASSWORD_TOO_SHORT` / `PASSWORD_TOO_LONG` / `CURRENT_PASSWORD_INCORRECT` / `DISPLAY_NAME_REQUIRED` / `DISPLAY_NAME_TOO_LONG`） |
| `FORBIDDEN` | 403 | 角色档位不足 / 向下管理越界 / 非超管改角色 |
| `SELF_MANAGEMENT_FORBIDDEN` | 403 | 删自己 / 禁自己 / 改自己角色 |
| `LAST_SUPERADMIN` | 409 | 最后一名启用超管被降级 / 禁用 / 删除 |
| `REFRESH_INVALID` | 401 | refresh 缺失 / 不存在 / 过期 / 被重用（族随之撤销） |
| `NOT_FOUND` | 404 | 路径用户不存在 |
| `INTERNAL_ERROR` | 500 | 兜底 |

### 5.3 端点清单

### auth

| 端点 | 请求 | 成功 | 备注 |
|---|---|---|---|
| `POST /auth/login` | `{email, password}` | `200 {accessToken, user}` + Set-Cookie | 见 §4.2 登录时序 |
| `POST /auth/refresh` | cookie + header | `200 {accessToken, user}` + 轮换 Set-Cookie | 不设 `GET /auth/me`：守卫水合只靠 refresh，一次往返 |
| `POST /auth/logout` | cookie + header | `204` | 幂等 |
| `PATCH /auth/password` | `{currentPassword, newPassword}` | `204` | 撤销全部族；当前密码错 → 422 字段级 |

### users

| 端点 | 权限 | 备注 |
|---|---|---|
| `GET /users` | admin+（user 档 403） | 查询参数见 §5.4 |
| `POST /users` | 向下 | `{email, displayName, role}` → `201 {user, initialPassword}`（初始密码一次性展示）；email 重复 409 `EMAIL_TAKEN`；`role` ≥ 自己档 → 403 |
| `PATCH /users/:id` | 向下 | `{displayName?, role?, isEnabled?}` → `200 {user}`；`role` 仅超管可改；目标与新角色须严格低于操作者（§3）；目标是自己 → 403 `SELF_MANAGEMENT_FORBIDDEN`；致最后一名启用超管被降/禁 → 409 `LAST_SUPERADMIN` |
| `DELETE /users/:id` | 仅超管 | `204`；自己 / 最后超管 → 403 / 409 同上 |
| `POST /users/:id/password-reset` | 向下 | `200 {initialPassword}`；置回 `mustChangePassword=true`、撤销该用户全部族；**不动 `isEnabled`**（重置 ≠ 解禁） |
| `PATCH /users/me` | 本人 | `{displayName}` → `200 {user}`（settings 自我资料，避免与向下管理纠缠） |

无单用户详情端点——编辑对话框数据取自列表行。

### 5.4 列表查询参数

`page`（1 起）· `pageSize`（默认 10，上限 100）· `sort`（单列，白名单 `email / displayName / role / isEnabled / createdAt / updatedAt`，方向 `asc|desc`，默认 `createdAt.desc`）· `role`（可重复 `role=admin&role=user`）· `status`（`enabled|disabled` 单值）· `q`（email / displayName 模糊，ILIKE，转义 `%` `_`）→ `200 { "data": [User], "meta": { "total", "page", "pageSize" } }`。

## 6. 前端改造清单

模板侦察事实（v2.2.1，已核实）：**无路由守卫**（重构时移除）；登录是 `sleep()` mock 写 Zustand `useAuthStore`（cookie persist、JSON 化 token、硬编码 key）；React Query 全局已配好；无 i18n；users 表格分页/排序/筛选全在 URL search params。

### 6.1 fork 与裁剪

- fork satnaing/shadcn-admin v2.2.1 → `frontend/`（剥离原 git 历史）。
- **删除**：tasks / chats / apps / help-center 路由、菜单项与组件；clerk 及第三方登录 demo；无关 settings 子 tab。
- **保留**：dashboard、users、sign-in、settings，403 / 500 错误页（403 页模板已有、未接线，接上）。
- 类型改造：`AuthUser` 字段统一为后端形状（`displayName` 等），`role` 在 store 层包 `[role]`。

### 6.2 认证接缝（ADR 0003）

1. **`src/lib/axios.ts` 新建实例**：`baseURL: '/api'`；请求拦截器注入 `Authorization: Bearer`（读 store）+ `X-Requested-With`；响应拦截器 401 → **单飞 refresh + 重放一次**（重放再 401 不再试）；refresh 失败清 store 跳 `sign-in?redirect=`。
2. **多标签页竞态**：refresh 调用包 `navigator.locks.request('auth-refresh', …)` 跨标签页串行（否则两标签页同时过期会并发刷新，一个被重用检测判死、整族撤销两边下线——ADR 0003）。后端保持纯轮换。
3. **`main.tsx`**：queryCache 的 401 分支删除（拦截器接管）；500→/500、mutation toast 保留。
4. **auth store 重写**：user + accessToken 仅内存；**整体删除 cookie persist 中间件**（顺除硬编码 key `'thisisjustarandomstring'`）。硬刷新由守卫先 refresh 再水合。
5. **sign-in form**：`sleep()` mock → `POST /auth/login`；zod 密码下限 7→8（对齐 8–128）；右上角语言切换器。
6. **`_authenticated/route.tsx` 补 `beforeLoad` 三层**：① 无 token → refresh 水合，失败 redirect `sign-in?redirect=`；② `mustChangePassword` → 一律去 settings/password（该页与 sign-in 除外）；③ 路由 `staticData` 角色档位检查（superadmin=3 ⊃ admin=2 ⊃ user=1），不足渲染 403 页。
7. **sidebar 菜单按角色过滤**：与 ③ 同一张静态「角色→可见路由」表（单一事实源）；users 项对 user 档隐藏。

### 6.3 users 页

- URL search params schema 改为 `page / pageSize / sort / role[] / status / q`；排序从本地 state 搬进 URL；`useQuery(['users', search])` 服务端分页。
- 列：displayName / email / role（前端渲染角色显示名：超级管理员/管理员/普通用户）/ status（isEnabled）；日期列随界面语言格式化。
- 操作列按角色显隐：删除仅超管；编辑/禁用按向下管理。
- 四个 dialog：create（**初始密码一次性展示** + 复制按钮，不落任何 store）/ edit / delete（确认）/ password-reset（同样一次性展示）。
- 模板的 username 列过滤与 4 值 status 随字段裁移除。

### 6.4 settings 页（三 tab）

- **profile**：`PATCH /users/me`，仅 displayName。
- **password**：`PATCH /auth/password`；首登强制改密的目的地；改完引导重登。
- **appearance**：主题（模板已有，保留）+ **语言切换器**（与登录页同一实现）。
- 模板其余 tab 裁掉。

### 6.5 i18n（ADR 0002）

- i18next + react-i18next + browser-languagedetector；检测链 **localStorage 记忆 → 浏览器语言 → 回退 `zh-CN`**；不进 cookie。
- `locales/zh-CN.json` 与 `locales/en.json` **从第一天同时提交，禁止组件内裸字符串**（约定 + review 把关 + CI 对齐脚本，不上 lint 工具）。
- **错误码 → locale 映射表**（`errors` namespace）：消费 §5.2 错误码与字段级码，无映射回退 `title`；角色显示名同归 locale。
- 日期随界面语言（zh → `2026-11-06 14:30`，en → `Nov 6, 2026`，date-fns + locale）；数字默认；**不引 CJK web font**（系统字体回退）。

## 7. 测试与 CI 基线

（决议 #11，ADR 0004——完整范围与理由见该 ADR。）

- **后端集成测试**：`#[sqlx::test]` 每测试一座临时库（自动迁移 + 注入 PgPool）+ `tower::ServiceExt::oneshot()` 进程内发请求；每条测试 = API 契约一行断言。五类：① auth 时序（登录→轮换→重用检测整族撤销→多族并存→登出撤销当前族→改密撤销全部族→禁用后 refresh 被拒）② RBAC 矩阵（3 角色 × 全端点表驱动 200/403）③ 护栏（自删/自禁/动自己角色/最后超管）④ users CRUD + 列表契约（`{data, meta}`、一次性初始密码）⑤ 首登改密链。种子：Rust helper 传参 + 预计算 argon2 哈希 const（零哈希开销）。
- **单元测试**只测纯逻辑：错误码映射、JWT claims 构造、分页/排序解析与 sort 白名单、初始密码生成器、argon2 round-trip。
- **前端零测试基建**：门禁 = eslint + `tsc -b` + `vite build` + `scripts/check-locales.mjs`（守护两份 locale 同天提交）；拦截器 refresh 逻辑手测，回归再补。
- **CI 双 job**（`.github/workflows/`，PR + push main，paths 过滤 `backend/**` / `frontend/**`，根级变更两者都跑）：
  - backend：`fmt --check` → `clippy --all-targets -D warnings` → `sqlx migrate run` + `prepare --check` → `cargo test`（PG service container `postgres:18-alpine`）；Swatinem/rust-cache（key 含 `.sqlx/`）。
  - frontend：lint + tsc + build + locale 脚本；setup-node npm 缓存 + `npm ci`。

## 8. 实施顺序

每个里程碑附出口条件；测试与实现同写（ADR 0004），不设独立「补测试」阶段。

| # | 里程碑 | 内容 | 出口条件 |
|---|---|---|---|
| M0 | 仓库骨架 | monorepo 布局；fork + 裁剪模板（§6.1）；`rust-toolchain.toml`；compose 起 PG 18；`.env` 样例 | 裁剪后前端 `npm run dev` 可跑（mock 登录暂留）；`cargo build` 过 |
| M1 | 数据层 | 迁移 0001 / 0002（§2）；seed CLI；`state.rs` / `error.rs`（错误码映射 + 单测） | `seed-superadmin` 跑通并打印一次性密码 |
| M2 | auth 模块 | login / refresh（轮换 + 重用检测 + 重读用户行）/ logout / password；JWT 签发校验；集成测试类① | 五条 auth 时序测试绿；手测登录全链路 |
| M3 | users 模块 | CRUD + password-reset + /users/me + 列表查询；`require_role` + 向下管理 + 护栏；集成测试类②③④⑤ + 单测 | 契约逐行断言绿；Swagger（dev）可见全部端点 |
| M4 | 前端认证接缝 | i18n 骨架 + 两份 locale；axios 拦截器 + Web Locks；auth store 重写；sign-in 真登录 | 真实登录 / 刷新 / 登出 / 硬刷新水合全链路通 |
| M5 | 守卫与页面 | beforeLoad 三层；sidebar 过滤；users 页（§6.3）；settings 三 tab；语言切换器 | 三角色各视角走查：可见性、403、首登强制改密、初始密码一次性展示 |
| M6 | CI 与门禁 | `check-locales.mjs`；双 workflow；`.sqlx` 落库 | 两 job 在 PR 上全绿 |
| M7 | 部署基线 | §9：多阶段 Dockerfile + 生产 compose + 同源伺服 | compose 一键起，浏览器走通登录→管理全流程 |

依赖关系：M1→M2→M3 串行；M0 与 M1 可并行；M4 依赖 M2（需要真后端联调）；M5 依赖 M3+M4；M6 依赖 M3+M5；M7 收尾。

## 9. 运行与部署基线

（地图雾区「部署与运行形态」由本 spec 吸收为基线：语义已由 ADR 0001/0003/0004 定死（同源、`/api`、PG 18），此处只固化机械参数，非阻塞、可随基础设施调整。）

### 9.1 开发环境

```yaml
# docker-compose.yml（根；dev 只起数据库）
services:
  db:
    image: postgres:18-alpine
    environment:
      POSTGRES_USER: rs_admin
      POSTGRES_PASSWORD: rs_admin
      POSTGRES_DB: rs_admin
    ports: ["5432:5432"]
    volumes: [pgdata:/var/lib/postgresql]
volumes: { pgdata: {} }
```

> postgres 18 镜像改为在 `/var/lib/postgresql` 下按主版本建数据目录，挂载点须为该父目录
> （挂旧路径 `/var/lib/postgresql/data` 会被 18 镜像拒绝启动）；spec 原文已据此修正。

- 后端：`cargo run`（启动自动 `sqlx::migrate!`），监听 `127.0.0.1:3000`。
- 前端：`npm run dev`（Vite 5173），proxy `'/api' → http://localhost:3000`。
- **Secure cookie on localhost**：现代 Chrome / Firefox 对 localhost 接受 `Secure` cookie；若团队浏览器异常，以 `localhost`（非 IP）访问或给 Vite 开 https。

### 9.2 环境变量

| 变量 | 说明 | dev 缺省 |
|---|---|---|
| `DATABASE_URL` | `postgres://rs_admin:rs_admin@localhost:5432/rs_admin` | 同左（`.env`） |
| `JWT_SECRET` | ≥32 字节随机（`openssl rand -base64 48`）；生产走 secret 注入 | `.env` |
| `APP_ENV` | `development` / `production`：tracing 输出（pretty/JSON）、Swagger 挂载开关 | `development` |
| `PORT` | 监听端口 | `3000` |
| `RUST_LOG` | `info,sqlx=warn` 量级 | `info` |

### 9.3 生产基线

- **同源**（ADR 0001）：多阶段 Dockerfile——node 构建前端 → cargo 构建后端 → 运行镜像内 axum 以 `tower-http` `ServeDir` 伺服 `frontend/dist` + SPA 回退 `index.html`，API 与静态同端口；反代方案等价可选。
- compose：`db`（postgres:18-alpine，与 CI 对齐）+ `backend`；`JWT_SECRET`/`DATABASE_URL` 走 secret/env 注入。
- 不配 CORS（同源）；Swagger 不挂载（`APP_ENV=production`）。
- K8s、TLS 终止、备份、监控等超出本基线，属后续运维 effort。
