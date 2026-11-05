# 调研：Axum + PostgreSQL + JWT 生态现状（截至 2026-11-05）

- **Ticket**: [#8](https://github.com/nasa-du/rs-admin-glm/issues/8)（wayfinder:research），服务于「后端选型」ticket #5。
- **方法**: 三个并行检索（数据层 / 认证 / 中间件与结构），全部结论追溯到一手来源（crates.io API、docs.rs、官方 GitHub 仓库与 CHANGELOG、RFC、OWASP Cheat Sheet）；关键版本号另由主线于 2026-11-05 直接对 crates.io API (`https://crates.io/api/v1/crates/<name>`) 交叉复核。未能在今日核实的点显式标注 **未核实**。
- **存放位置**: 仓库此前无 research 笔记目录约定（仅有 `docs/agents/` 与规划中的 `docs/adr/`），故落在 `research/axum-ecosystem.md`。

## TL;DR 推荐栈（供 #5 直接消费）

| 用途 | Crate | 版本（发布日期，crates.io） | 备注 |
|---|---|---|---|
| Web 框架 | `axum` | **0.8.9**（2026-04-14） | 0.8 仍是当前稳定大版本；MSRV 1.80；注意路径语法 `{id}` |
| 数据层 | `sqlx` | **0.9.0**（2026-05-21） | 社区默认选择；`PgPool` 直接作为 axum State；编译期校验 `query!` |
| 迁移 | `sqlx-cli` / `sqlx::migrate!` | 0.9.0（2026-05-21） | 注意 0.9 起 `cargo install --locked sqlx-cli` 不可用 |
| JWT | `jsonwebtoken` | **11.1.0**（2026-09-16） | 单体服务 HS256；10.0 起必须显式选 crypto backend feature |
| 密码哈希 | `argon2` | **0.6.0**（2026-08-27） | `Argon2::default()` 即满足 OWASP（Argon2id, m=19 MiB, t=2, p=1） |
| 中间件 | `tower-http` | **0.7.1**（2026-08-31） | CORS / Trace / Compression / Timeout |
| OpenAPI（可选） | `utoipa` + `utoipa-axum` + `utoipa-swagger-ui` | **6.0.0** / 0.3.0 / 10.0.2 | OpenApiRouter + `routes!` 为当前推荐写法 |

**工具链提示**: sqlx 0.9 与 SeaORM 2.0 均要求 Rust ≥ 1.94（edition 2024），比 axum 0.8.9 自身的 MSRV（1.80）高——**整条栈按 ≥1.94 选定工具链即可全覆盖**。

---

## 1. SQLx vs SeaORM vs Diesel

### 1.1 版本与维护活跃度

| Crate | 当前稳定版 | 发布节奏（近 12 个月） | 维护读数 |
|---|---|---|---|
| `sqlx` | 0.9.0（2026-05-21） | 0.8.6（2025-05）→ 0.9.0-alpha（2025-10）→ 0.9.0；此后 6 个月无 stable 补丁，但 main 上修复持续合并（最近 commit 2026-10-27~31） | 活跃；**仓库已从 LaunchBadge 迁至 [transact-rs org](https://github.com/transact-rs/sqlx)**，长期维护者 abonander 仍负责发版（0.9 CHANGELOG 明言 "SQLx is no longer owned by LaunchBadge, LLC"） |
| `sea-orm` | 2.0.4（2026-09-27） | 2.0.0-rc 双周更（rc.27 2025-12 → rc.43 2026-07）→ 2.0.0（2026-07-19）→ 10 周内 4 个补丁 | 非常活跃（SeaQL / Chris Tsang）；2.0 为大版本重构 |
| `diesel` | 2.3.14（2026-10-07） | 2.3.0（2025-09）起约每月一个补丁 | 最稳的节奏（weiznich）；2.2.x→2.3 经过第三方安全审计（Radically Open Security，NLnet 资助，2025-10） |

近期下载量（crates.io recent 窗口，比例参考而非调查）：sqlx ≈ 5410 万 > diesel ≈ 1130 万 > sea-orm ≈ 590 万。
来源: [crates.io/sqlx](https://crates.io/api/v1/crates/sqlx) · [crates.io/sea-orm](https://crates.io/api/v1/crates/sea-orm) · [crates.io/diesel](https://crates.io/api/v1/crates/diesel) · [sqlx CHANGELOG](https://raw.githubusercontent.com/transact-rs/sqlx/main/CHANGELOG.md) · [SeaORM releases](https://api.github.com/repos/SeaQL/sea-orm/releases?per_page=8) · [diesel.rs](https://diesel.rs/)

**Diesel 3 / diesel_next**: 无任何公开证据（无 GitHub milestone、CHANGELOG 无 3.0 段落、diesel.rs 无提及）；diesel.rs 下一个版本是 **2.4**（changelog Unreleased：batch updates、`#[derive(diesel::Enum)]`、PG `RETURNING old.col` 需 PG18、MSRV 升 1.88）。`diesel_next` 在 crates.io 上 **不存在**（404）。→ 对选型意味着：Diesel 路线没有迫近的大版本风险，也没有「Diesel 3 一步到位」的选项。

### 1.2 与 Axum 0.8 的集成惯用法

- **SQLx（axum 官方仓库自带 first-party 示例 [`examples/sqlx-postgres`](https://github.com/tokio-rs/axum/tree/main/examples)）**: `PgPool` 内部已 Arc，**clone 廉价，可直接 `Router::with_state(pool)`**；handler 用 `State(pool): State<PgPool>`，或自定义 extractor 走 `PgPool::from_ref(state)` + `pool.acquire().await`（示例中为 RPITIT 风格，无 `#[async_trait]`）。编译期校验 `sqlx::query!` / `query_as!`（需 `DATABASE_URL` 或 `.sqlx` 离线缓存）；运行时拼接 SQL 用 `sqlx::query(...)`，**0.9 起动态 SQL 须包 `AssertSqlSafe`**（`query*()` 参数改为 `impl SqlSafeStr`）。来源: [examples/sqlx-postgres/src/main.rs](https://raw.githubusercontent.com/tokio-rs/axum/main/examples/sqlx-postgres/src/main.rs)
- **SeaORM（官方自带 [`examples/axum_example`](https://github.com/SeaQL/sea-orm/tree/master/examples/axum_example)）**: `Database::connect(url)` → `DatabaseConnection`（内部包 sqlx pool，Clone）放进 `#[derive(Clone)] struct AppState`，handler 取 `State<AppState>`；启动时 `Migrator::up(&conn, None)`。查询为运行时 query-builder（无编译期 SQL 校验，类型安全来自生成的 entity 结构体）。来源: [axum_example/src/lib.rs](https://raw.githubusercontent.com/SeaQL/sea-orm/master/examples/axum_example/api/src/lib.rs)
- **Diesel（axum 官方仓库两个示例）**: 同步路线 [`examples/diesel-postgres`](https://github.com/tokio-rs/axum/tree/main/examples/diesel-postgres) 用 **deadpool-diesel**，`conn.interact(|conn| ...)` 派发阻塞调用；异步路线 [`examples/diesel-async-postgres`](https://github.com/tokio-rs/axum/tree/main/examples/diesel-async-postgres) 用 **diesel-async 0.9.2（2026-06-19）+ bb8**（`AsyncDieselConnectionManager`），`.with_state(pool)` + `pool.get().await`。来源: [diesel-async](https://crates.io/api/v1/crates/diesel_async) · [diesel_async README](https://raw.githubusercontent.com/weiznich/diesel_async/main/README.md)
- 值得注意: axum 官方 examples 含 sqlx-postgres、diesel-postgres、diesel-async-postgres、tokio-postgres、mongodb、tokio-redis，**没有 SeaORM 示例**（SeaORM 自己维护）。来源: [examples 目录](https://api.github.com/repos/tokio-rs/axum/contents/examples)

### 1.3 迁移工具链

- **SQLx**: `sqlx-cli 0.9.0` + `sqlx::migrate!()`（`migrate` feature 默认开启）。流程 `cargo sqlx migrate add/run` + 启动时 `migrate!().run(&pool)` + `cargo sqlx prepare` 生成 CI 用的 `.sqlx` 离线缓存。0.9 新增 per-crate `sqlx.toml` 配置（CLI 默认启用）。**坑**: 0.9 起仓库不再提交 `Cargo.lock`，`cargo install --locked sqlx-cli` 会失败；CLI 默认 native-tls。来源: [sqlx CHANGELOG](https://raw.githubusercontent.com/transact-rs/sqlx/main/CHANGELOG.md) · [crates.io/sqlx-cli](https://crates.io/api/v1/crates/sqlx-cli)
- **SeaORM**: `sea-orm-cli` + `sea-orm-migration`（均 2.0.4）。`sea-orm-cli migrate generate` 生成 `mYYYYMMDD_*.rs`（`MigrationTrait` 的 up/down，SeaQuery DSL），CLI 可跑也可应用内 `Migrator::up`；`sea-orm-cli generate entity` 从库反向生成实体；PG 上迁移事务性。来源: [SeaORM 迁移文档](https://www.sea-ql.org/SeaORM/docs/migration/writing-migration/)
- **Diesel**: `diesel_cli 2.3.14`，SQL 文件式（up.sql/down.sql）+ `diesel print-schema` 产出 `schema.rs`；应用内 `embed_migrations!`（同步）/ `AsyncMigrationHarness`（diesel-async）。来源: [docs.rs/diesel_cli](https://docs.rs/crate/diesel_cli/latest)

### 1.4 社区倾向（2025–2026，新 Axum 服务）

多路证据一致指向 **「SQL-first 异步服务默认 SQLx；要完整 ORM 才选 SeaORM；Diesel 占类型安全/同步生态位但 via diesel-async 完全可用」**:

- axum 官方 examples 的取舍（见上）是维护者眼中主流的合理代理。
- [Are We Web Yet](https://www.arewewebyet.org/)（2025-07 更新）首页列 Diesel 与 sqlx；SeaORM 只出现在 [Database 主题页](https://www.arewewebyet.org/topics/database/)，无成熟度排名。
- 下载比 ≈ 5:2:1（sqlx:diesel:sea-orm，见 1.1；下载量是代理指标不是调查）。
- 模板/框架生态: SeaORM 侧有 Loco（Rails-like，axum+SeaORM，2026-07 达 1.0）[crates.io/loco](https://crates.io/api/v1/crates/loco)；SQLx 侧的经典 starter `launchbadge/realworld-axum-sqlx`（1115 星）**已停更（最后 push 2023-12，且钉在 axum 0.3）**——说明 SQLx 的「默认地位」不等于「有维护良好的官方模板」（当前可用的模板见 §5.3）。
- 三方对比文章结论一致（Rustfinity、Shuttle 等，二手来源仅作社区声音）: [rustfinity.com/blog/rust-orms](https://www.rustfinity.com/blog/rust-orms) · [shuttle.dev blog](https://www.shuttle.dev/blog/2024/01/16/best-orm-rust)

### 1.5 实践注意事项（新项目会踩的）

- **运行时 feature**: sqlx 0.9 必须显式选 runtime（axum 用 `features = ["runtime-tokio"]` + TLS 组合）；SeaORM 2.0 需 `runtime-tokio` + `sqlx-postgres` + TLS。来源: [docs.rs sqlx features](https://docs.rs/crate/sqlx/0.9.0/features) · [docs.rs sea-orm features](https://docs.rs/crate/sea-orm/2.0.4/features)
- **TLS feature 命名是最大升级坑**: sqlx 0.9 **删除**了 `runtime-tokio-native-tls` / `runtime-tokio-rustls` 这类组合 feature，改为 `tls-native-tls` / `tls-rustls` / `tls-rustls-ring` / `tls-rustls-aws-lc-rs` 等；SeaORM 2.0 仍叫 `runtime-tokio-rustls`（底层映射 sqlx 的 `tls-rustls`）。同应用混用须保持 crypto provider（ring vs aws-lc-rs）一致，否则 build/运行时报 provider 不匹配。来源: [sqlx CHANGELOG](https://raw.githubusercontent.com/transact-rs/sqlx/main/CHANGELOG.md)
- **MSRV 阶梯**: axum 0.8.9 = 1.80 < utoipa 6 = 1.88 < jsonwebtoken 11 = 1.88 ≈ diesel 2.4(unreleased) = 1.88 < sqlx 0.9 / SeaORM 2.0 = **1.94**（edition 2024）。
- Diesel 的 TLS 基本自助（diesel-async README: `sslmode=require` 需自己构建 TLS，仓库有 rustls 示例）。

---

## 2. jsonwebtoken crate

### 2.1 版本与维护

- 当前稳定 **11.1.0（2026-09-16）**；近线: 9.3.1（2025-02）→ 10.0.0（2025-09-29）→ 10.3/10.4（2026-01/05）→ 11.0.0（2026-07-24）→ 11.1.0。无 yanked。MSRV 1.88、edition 2024。维护者 Vincent Prouillet（Keats），仓库最后 push 2026-10-04，活跃。来源: [crates.io](https://crates.io/api/v1/crates/jsonwebtoken) · [GitHub](https://api.github.com/repos/Keats/jsonwebtoken) · [CHANGELOG](https://raw.githubusercontent.com/Keats/jsonwebtoken/master/CHANGELOG.md)
- **大版本断点**: 10.0 起 crypto trait 化，**必须二选一 backend feature: `aws_lc_rs` 或 `rust_crypto`**（否则自供 `CryptoProvider`）；纯 Rust 线选 `rust_crypto`。11.0: `Algorithm` 等 `#[non_exhaustive]`、移除 `insecure_disable_signature_validation`（改 `dangerous::insecure_decode*`）、`Jwk.thumbprint` 返回 `Result`。10.4: 密钥 drop 时部分 zeroize。

### 2.2 API 形态（11.1.0）

- 顶层 `encode` / `decode` / `decode_header`；`EncodingKey::from_secret`（HMAC）/ `from_rsa_pem`、`from_ed_pem` 等（非对称，PEM 走默认 `use_pem` feature）；`DecodingKey` 另有 **`from_jwk(&Jwk)`**。
- `Validation` 字段（默认值）: `required_spec_claims = {"exp"}`、`leeway = 60`、`validate_exp = true`、`validate_aud = true`、`algorithms = [HS256]`、`aud/iss/sub: Option<...>`；`Validation::new_for_family(AlgorithmFamily)`（10.4+）可按算法族校验。**注意: 不存在 `RequiredSpecClaims` 枚举**——字段一直是 `HashSet<String>`（9.x/8.x/7.x 均核实过 docs.rs）。
- 密钥对象应**启动时构造一次**复用（README 明确建议），勿每请求重 parse PEM。
- 来源: [docs.rs index](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/index.html) · [Validation](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/struct.Validation.html) · [DecodingKey](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/struct.DecodingKey.html) · [README](https://raw.githubusercontent.com/Keats/jsonwebtoken/master/README.md)

### 2.3 HS256 vs RS256 惯例

- **单体服务（同一服务签发+校验，本项目的后台形态）→ HS256**：长随机 secret 来自 env/配置，快且简单（README 默认即 HS256）。
- **多服务/第三方校验 → RS256/ES256/EdDSA**：只给验证方公钥。EC 私钥须 PKCS8。
- **key rotation / kid / JWKS: crate 不内置 JWKS 拉取与 kid 分发**，惯用做法是 `decode_header`（不验签读 `alg`/`kid`）→ 查本地 `jwk::JwkSet` → `DecodingKey::from_jwk` → `decode`。
- 来源: [README](https://raw.githubusercontent.com/Keats/jsonwebtoken/master/README.md) · [jwk module](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/jwk/index.html) · 社区惯例二手佐证: [supertokens](https://supertokens.com/blog/rs256-vs-hs256) / [auth0](https://auth0.com/blog/rs256-vs-hs256-whats-the-difference)

### 2.4 刷新令牌常用做法

- **轮换 + 重用检测已是成文标准**: RFC 9700（OAuth 2.0 Security BCP，2025-01 发布）§4.14.2 明文要求「每次刷新同时发新 refresh token；旧 token 失效但保留关系；重用即视为泄露，撤销整个 token family」。来源: [RFC 9700](https://datatracker.ietf.org/doc/html/rfc9700) · [Auth0 refresh token rotation](https://auth0.com/docs/secure/tokens/refresh-tokens/refresh-token-rotation)
- **典型参数**: 短寿命 access JWT（常见 15 min）+ 长寿命 refresh（示例: 4 周），如 [dev.to Axum JWT 系列](https://dev.to/bittokks/implementing-jwt-authentication-in-rust-using-axum-4bm4)（RS256、access exp=900s、refresh exp=2419200s）。
- **传输**: 浏览器端 refresh token 放 `HttpOnly; Secure; SameSite=Strict`（OWASP Session Management 建议外加 `__Host-` 前缀）cookie；API/服务端用 `Authorization: Bearer`。来源: [OWASP Session Management Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
- **存储**: refresh token 存 DB 时哈希（如 SHA-256）后落库 + 记录 family/轮换链。可参考的 Axum 实现: [sheroz.com Axum+SQLx+Redis JWT](http://sheroz.com/pages/blog/rust-axum-rest-api-postgres-redis-jwt-docker.html)（轮换+Redis 黑名单）、[wpcodevo/rust-axum-jwt-rs256](https://github.com/wpcodevo/rust-axum-jwt-rs256)。**未核实**: 未找到一篇把「Postgres 哈希存储 + 轮换 + 重用检测」完整写穿的权威 Rust 文章——组合方案需按 RFC 9700 自行实现，这是 #5 实现时的一个工作量项。

---

## 3. argon2 密码哈希

### 3.1 版本现状

- 当前稳定 **0.6.0（2026-08-27）**，经 0.6.0-rc.0（2025-05）至 rc.8（2026-03）的漫长 RC 线；生态大头仍在 0.5.3（2024-01，下载最多）。RustCrypto/password-hashes monorepo 最后 push 2026-11-05，活跃；MSRV 1.85。来源: [crates.io](https://crates.io/api/v1/crates/argon2) · [GitHub](https://api.github.com/repos/RustCrypto/password-hashes)
- **0.5 → 0.6 API 变化（网上多数教程还是 0.5 写法）**: PHC 类型拆到独立 `phc` crate；`SaltString::generate()` **不再接收 `&mut OsRng` 参数**（`generate() -> Self`，getrandom feature）；`PasswordHasher` 的必实现方法变为 `hash_password_with_salt`，提供的 `hash_password(&self, password)` **自动生成大随机盐**——最省事的做法就是直接 `argon2.hash_password(password.as_bytes())`。`verify_password` 按存的 PHC 字符串里的参数校验（升级参数后旧哈希仍可验，可在下次登录重哈希）。来源: [docs.rs argon2 0.6.0](https://docs.rs/argon2/0.6.0/argon2/index.html) · [PasswordHasher](https://docs.rs/password-hash/0.6.1/password_hash/trait.PasswordHasher.html) · [phc::SaltString](https://docs.rs/phc/latest/phc/struct.SaltString.html)

### 3.2 推荐参数（OWASP，2026-11 实时核对）

OWASP Password Storage Cheat Sheet 给出等强度的 Argon2id 最低配置（CPU/RAM 权衡）:

| m（内存） | t（迭代） | p | 备注 |
|---|---|---|---|
| 47104 KiB（46 MiB） | 1 | 1 | 内存充裕时 |
| **19456 KiB（19 MiB）** | **2** | **1** | **默认推荐** |
| 12288 KiB（12 MiB） | 3 | 1 | 内存紧张 |
| 9216 KiB（9 MiB） | 4 | 1 | |
| 7168 KiB（7 MiB） | 5 | 1 | |

- 前两行标注「勿用于 Argon2i」（Argon2id 无碍）；p 只是 lanes，增大 p 不增加总内存也不等效迭代数。
- **RustCrypto 默认值与 OWASP 推荐完全一致**: 源码 `DEFAULT_M_COST = 19*1024`、`DEFAULT_T_COST = 2`、`DEFAULT_P_COST = 1`，`Argon2::default()` 即 Argon2id v19 + 该参数组——**新项目直接用默认即可达标**，仅在做服务器调优时用 `Params::new(m, t, p, output_len)`。
- 服务器调优: OWASP 要求单次哈希 < 1 秒并在目标机器实测；本服务（管理后台，登录并发低）按 ~19 MiB × 并发登录估算内存预算即可，注意登录端口的 CPU-DoS（配合限流）。
- 来源: [OWASP Password Storage Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) · [params.rs 源码](https://docs.rs/argon2/0.6.0/src/argon2/params.rs.html)

### 3.3 对比 bcrypt（一段话）

OWASP 定位 Argon2 为首选、bcrypt 仅限遗留兼容；Rust 的 `bcrypt` crate 仍健康（0.19.3，2026-07-27，与 jsonwebtoken 同一维护者）。bcrypt 无内存硬度且截断 72 字节输入，新设计不选；仅在兼容存量哈希时用。来源: [OWASP](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) · [crates.io/bcrypt](https://crates.io/api/v1/crates/bcrypt)

---

## 4. tower-http 与 utoipa 集成

### 4.1 tower-http 0.7.1（2026-08-31）

- 0.7.0（2026-06-15）新增 `CsrfLayer`、请求/响应体 DeadlineLayer、ServeDir 强 ETag 等；与 axum 0.8 同居 http 1.x，直接 `.layer(...)` 互操作。MSRV 1.65。来源: [CHANGELOG](https://github.com/tower-rs/tower-http/blob/main/tower-http/CHANGELOG.md) · [crates.io](https://crates.io/api/v1/crates/tower-http)
- **CORS**（官方 [cors example](https://github.com/tokio-rs/axum/blob/main/examples/cors/src/main.rs)）: `CorsLayer::new().allow_origin(...).allow_methods([...]).allow_headers([CONTENT_TYPE])`——带 JSON body 的跨域请求**必须 allow CONTENT_TYPE**，否则预检失败。
- **Trace**（官方 [tracing-aka-logging example](https://github.com/tokio-rs/axum/blob/main/examples/tracing-aka-logging/src/main.rs)）: `TraceLayer::new_for_http()` + `.make_span_with(...)`（`info_span!` 记录 method/matched_path，取自 `MatchedPath` extension）+ `.on_request/.on_response`；EnvFilter 建议 `tower_http=debug,axum::rejection=trace`。0.7.0 起 trace 事件正确挂在请求 span 下。
- **Compression / Timeout**: `ServiceBuilder::new().layer(RequestDecompressionLayer::new()).layer(CompressionLayer::new())`（[compression example](https://github.com/tokio-rs/axum/blob/main/examples/compression/src/main.rs)）；`TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(30))`（不改 error 类型）。
- **分层顺序语义**（[Router::layer docs](https://docs.rs/axum/0.8.9/axum/routing/struct.Router.html#method.layer)）: `Router::layer` 只作用于已添加的路由、在路由之后运行（不能改 URI）；鉴权类早退中间件用 `route_layer`（避免把 404 变 401）；`ServiceBuilder` 中**先加的 layer 先看到请求**（[tower docs](https://raw.githubusercontent.com/tower-rs/tower/master/tower/src/builder/mod.rs)）。

### 4.2 utoipa 6.0.0（2026-09-22）+ utoipa-axum 0.3.0 + utoipa-swagger-ui 10.0.2

- 6.0 相对 5.x: 支持 OpenAPI 3.2（`Version32`、webhooks、SSE 建模）、移除 `serde_norway`（yaml 走 `yaml_serde`）、MSRV 1.88。来源: [utoipa CHANGELOG](https://github.com/juhaku/utoipa/blob/master/utoipa/CHANGELOG.md)
- **Axum 0.8 支持**: 自 utoipa 5.3.1 / utoipa-axum 0.1.4 / swagger-ui 8.1.1（2025-01）起；当前 6.0 家族在 Cargo.toml 里钉 `axum = "0.8.4"`，完全匹配 axum 0.8.9。来源: [utoipa-swagger-ui Cargo.toml](https://raw.githubusercontent.com/juhaku/utoipa/master/utoipa-swagger-ui/Cargo.toml) · [utoipa-axum Cargo.toml](https://raw.githubusercontent.com/juhaku/utoipa/master/utoipa-axum/Cargo.toml)
- 两种声明方式（均现行）: (a) `#[utoipa::path]` + `#[derive(OpenApi)]` 手动挂 Router（[simple-axum](https://github.com/juhaku/utoipa/blob/master/examples/simple-axum/src/main.rs)）；(b) **推荐** `OpenApiRouter`（utoipa-axum）: `OpenApiRouter::with_openapi(ApiDoc::openapi()).routes(routes!(health)).nest("/api/...", ...)`, `split_for_parts()` 得 `(Router, OpenApi)`（[axum-utoipa-bindings example](https://github.com/juhaku/utoipa/blob/master/examples/axum-utoipa-bindings/src/main.rs)、[docs.rs](https://docs.rs/utoipa-axum/0.3.0/utoipa_axum/)）。
- UI: `.merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api))`；也可换 Scalar（utoipa-scalar 0.4.0）。swagger-ui 需开 `axum` feature（+ `vendored` 或 `reqwest` 拉取静态资源）。

---

## 5. Axum 0.8 惯用项目结构与参考

### 5.1 0.8 相对 0.7 的关键变化（写新代码前必知）

来源均为 [axum CHANGELOG](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md) 与 [axum-core CHANGELOG](https://github.com/tokio-rs/axum/blob/main/axum-core/CHANGELOG.md)：

- **路径参数语法 `/:id` → `/{id}`**（matchit 0.8 升级；旧语法直接 panic 以防静默行为变化）。这是最大迁移项。
- **async trait 转 RPITIT**: `FromRequestParts`/`FromRequest` 与 `middleware::from_fn` 为原生 `async fn` trait，自定义 extractor **不再需要 `#[async_trait]`**。
- `Option<T>` extractor 语义收紧（需 `OptionalFromRequest[Parts]`；`Option<Path<T>>` 不再吞所有错误）。
- 移除/移动: `Host` → axum-extra、`serve` 泛型化、handler 要求 `Sync` 等。
- MSRV 演进: 1.75（0.8.0）→ 1.78（0.8.5）→ **1.80（0.8.9）**；main 分支（未发布）已排队更多 break（`tower-log` 不再默认等）。

### 5.2 State 提取惯用法

docs.rs 官方指引列出四种共享方式（State extractor / request extensions / closure captures / task-local），并示例 `Arc<AppState>` + `Router::with_state`。实际两层用法: 纯 DB 场景官方 `sqlx-postgres` 示例直接 `with_state(pool)`（无 AppState struct）；需要捆绑更多依赖（DB + 配置 + 密钥）时用 `#[derive(Clone)] struct AppState { pool: PgPool, ... }`（Arc 或依赖内部 Arc 均可）。来源: [docs.rs "Sharing state with handlers"](https://docs.rs/axum/0.8.9/axum/#sharing-state-with-handlers) · [sqlx-postgres example](https://github.com/tokio-rs/axum/blob/main/examples/sqlx-postgres/src/main.rs)

### 5.3 高质量参考（全部于 2026-11-05 核对 manifest）

**官方**:

- [tokio-rs/axum examples](https://github.com/tokio-rs/axum/tree/main/examples) —— 与本服务最相关的: [`sqlx-postgres`](https://github.com/tokio-rs/axum/tree/main/examples/sqlx-postgres)、[`error-handling`](https://github.com/tokio-rs/axum/tree/main/examples/error-handling)、[`tracing-aka-logging`](https://github.com/tokio-rs/axum/tree/main/examples/tracing-aka-logging)、另有 `cors` / `compression` / `todos` / `validator` / `testing` / `versioning` / `request-id` / `graceful-shutdown`。（注意: 猜测名 `rest-grpc`/`sessions`/`login`/`postgres` 在当前树中**不存在**。）
- [docs.rs/axum](https://docs.rs/axum/0.8.9/axum/) 主文档（State 共享、Router::layer 语义）。

**模板仓库（已核对钉在 0.8）**:

- [`nakamuraos/axum-postgres-boilerplate`](https://github.com/nakamuraos/axum-postgres-boilerplate) —— **与本地图最契合**: axum 0.8.9 + tower-http 0.7.1 + utoipa 6.0.0 + sqlx/seaorm + **JWT + RBAC**；feature 分层: `src/modules/{auth,users,health}` + `src/common/`（config/errors/extractors/middlewares）+ `src/database/` + `src/app.rs`。
- [`yasinakmaz/Axum-Starter-Template`](https://github.com/yasinakmaz/Axum-Starter-Template) —— clean-architecture workspace（domain/application/infrastructure/presentation 四 crate + bin/api），axum 0.8.9。
- [`joelparkerhenderson/demo-rust-axum`](https://github.com/joelparkerhenderson/demo-rust-axum) —— axum ~0.8.4 的小例子集合。

**反面警示（常被误引）**: `launchbadge/realworld-axum-sqlx` 钉在 **axum 0.3**（2023 年停更）；`LukeMathWalker/zero-to-production` 是 **actix-web**，其 layout 常被抄进 axum 项目但仓库本身不可作 axum 参考。**「launchpad」模板未找到现存 axum 版本（未核实/疑不存在）**。

**关于 `routes/ + models/ + state.rs` 平铺结构**: 无权威仓库在 0.8 上使用该精确形态；当前可核实的权威形态是 (1) modules-per-feature（上面的 boilerplate，最接近 feature 分层惯用法）、(2) clean-architecture workspace、(3) 官方示例的极简 `with_state(pool)`。教程（如 [dev.to 2025-12 Axum+SQLx CRUD](https://dev.to/francescoxx/rust-crud-rest-api-using-axum-sqlx-postgres-docker-and-docker-compose-152a)）有平铺写法但非权威。

---

## 对 #5「后端选型」的 5 条决策性结论

1. **数据层选 SQLx 0.9**: 社区默认 + axum 官方 first-party 示例 + 编译期 SQL 校验 + `sqlx::migrate!` 一体化迁移；SeaORM 2.0 活跃但引入 ORM 抽象且无编译期 SQL 校验，Diesel 2.3 稳健但在 axum 异步语境需要额外一层（diesel-async/deadpool）。风险提示: sqlx 0.9.0 后 6 个月无 stable 补丁（修复积压在 main），且发版组织已换（transact-rs）——盯 0.9.1。
2. **JWT 选 jsonwebtoken 11.1 + HS256**: 单体后台自己签发自己校验，HS256 最简；10.0 起必须显式选 crypto backend（建议 `rust_crypto` 纯 Rust 线）。刷新令牌按 RFC 9700 做**轮换 + 重用检测 + 撤销 family**，refresh 存 DB 哈希；浏览器端 cookie 三件套 HttpOnly/Secure/SameSite。注意完整刷新方案无权威 Rust 范文，属自实现工作量。
3. **密码哈希 argon2 0.6 默认参数即达标**: `Argon2::default()` = Argon2id + m=19456/t=2/p=1，恰为 OWASP 推荐行；警惕网上 0.5 写法（`SaltString::generate(&mut OsRng)` 在 0.6 已变签名），或直接用自动加盐的 `hash_password`。
4. **中间件与文档零悬念**: tower-http 0.7.1（CORS 记得 allow CONTENT_TYPE；Trace 用官方 tracing example 模板）+ utoipa 6.0 家族（`OpenApiRouter` + `routes!`，钉 axum 0.8.4 兼容 0.8.9），Swagger UI/Scalar 即插即用。
5. **结构与工具链**: axum 0.8.9 的 `{id}` 路径语法与 RPITIT extractor 是写代码的基线认知；项目结构以 `nakamuraos/axum-postgres-boilerplate`（JWT+RBAC+utoipa，恰好同栈）为主参考 + 官方 `sqlx-postgres`/`tracing-aka-logging` 示例；**Rust 工具链按 ≥ 1.94 选定**（sqlx 0.9 的要求）。

## 未核实与已知缺口

- 「Postgres 哈希存储 + 轮换 + 重用检测」完整一体的权威 Rust 实现范文未找到（各文章各覆盖一部分）。
- sqlx 0.9.0 之后积累的修复何时发 0.9.1 无公开时间表。
- HS256/RS256 的「社区共识」部分来自身份厂商博客（supertokens/auth0/workos），与 crate 文档方向一致但为二手来源。
- crates.io 下载量为代理指标，非使用调查。

## 来源总览

一手: [crates.io API](https://crates.io/api/v1/crates/axum)（axum/sqlx/sea-orm/diesel/jsonwebtoken/argon2/tower-http/utoipa 及 cli 伴生包，2026-11-05 核对）· [axum CHANGELOG](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md) · [axum examples](https://github.com/tokio-rs/axum/tree/main/examples) · [docs.rs/axum 0.8.9](https://docs.rs/axum/0.8.9/axum/) · [sqlx CHANGELOG](https://raw.githubusercontent.com/transact-rs/sqlx/main/CHANGELOG.md) · [SeaORM docs](https://www.sea-ql.org/SeaORM/docs/migration/writing-migration/) · [diesel.rs](https://diesel.rs/) · [diesel_async](https://github.com/weiznich/diesel_async) · [jsonwebtoken docs.rs/CHANGELOG/README](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/index.html) · [argon2 docs.rs](https://docs.rs/argon2/0.6.0/argon2/index.html) · [RustCrypto/password-hashes](https://github.com/RustCrypto/password-hashes) · [tower-http CHANGELOG](https://github.com/tower-rs/tower-http/blob/main/tower-http/CHANGELOG.md) · [utoipa](https://github.com/juhaku/utoipa) · [RFC 9700](https://datatracker.ietf.org/doc/html/rfc9700) · [OWASP Password Storage](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) · [OWASP Session Management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
二手（社区声音）: [Are We Web Yet](https://www.arewewebyet.org/) · [Rustfinity](https://www.rustfinity.com/blog/rust-orms) · [sheroz.com](http://sheroz.com/pages/blog/rust-axum-rest-api-postgres-redis-jwt-docker.html) · [dev.to bittokks](https://dev.to/bittokks/implementing-jwt-authentication-in-rust-using-axum-4bm4) · [wpcodevo/rust-axum-jwt-rs256](https://github.com/wpcodevo/rust-axum-jwt-rs256) · [Auth0 rotation](https://auth0.com/docs/secure/tokens/refresh-tokens/refresh-token-rotation)
