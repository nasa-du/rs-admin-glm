# 测试策略与 CI 基线：真库集成 + 纯函数单测，前端零测试基建

后端把测试重量全部押在真 Postgres 集成层：`#[sqlx::test]` 给每个测试一座全新临时库（自动跑 migrations、注入 PgPool），Axum router 组装后用 `tower::ServiceExt::oneshot()` 进程内发请求——不起真端口、不 mock DB。每条集成测试对应 API 契约（ADR 0003）的一行断言，不追覆盖率数字；单元测试只留纯逻辑。前端不立组件测试与 e2e，门禁为静态检查 + build + locale 对齐脚本。CI 为 backend/frontend 双 job，PR 与 push main 双触发、paths 过滤。决议过程与逐项理由见 [#11](https://github.com/nasa-du/rs-admin-glm/issues/11)。

## Considered Options

- **testcontainers / 真起 TCP 端口发 HTTP（拒绝）**：`#[sqlx::test]` + `oneshot` 已保留真库语义（唯一约束、事务、时序），零额外基建、零端口协调。
- **mock-DB 的 handler 单元测试（拒绝）**：集成测试覆盖真实路径，mock 的编写与维护成本买不到额外信心。
- **vitest 组件测试（拒绝）**：shadcn-admin v2.2.1 零测试基建（package.json 无 test script，已核），管理后台 CRUD 组件多为模板已验证代码，ROI 低。axios 拦截器的单飞 refresh + Web Locks 是前端唯一复杂逻辑，暂手测覆盖，出现回归再补单测。
- **Playwright e2e（拒绝）**：dev 期 Vite proxy 直连后端 + 后端集成测试已近似覆盖同等路径，e2e 的维护成本在这个体量不值。
- **sqlx 内置 `.sql` fixtures（拒绝）**：RBAC 矩阵与护栏测试的用户组合逐测试不同，Rust seed helper 传参更灵活；标准测试密码的 argon2 哈希预计算为 const，seed 时零哈希开销。
- **第三方 setup-sqlx action / cargo-binstall（拒绝）**：CI 内 `cargo install sqlx-cli --version 0.9.0 --no-default-features --features rustls` 走官方渠道，接受 2–4 分钟编译，不引入第三方 action 信任面。
- **仅 PR 触发、不做 paths 过滤（拒绝）**：main 被破坏要立刻知道；monorepo 下 `backend/**` / `frontend/**` 互不触发可省无谓的 runner 时间。

## Consequences

- 集成测试覆盖五类，每类断言到契约行级：auth 时序（登录→轮换→重用检测整族撤销→多族并存→登出撤销当前族→改密撤销全部族→禁用后 refresh 被拒）、RBAC 矩阵（3 角色 × 全端点表驱动 200/403）、护栏（自删/自禁/动自己角色/最后一名启用超管）、users CRUD + 列表契约（`{data, meta}`）、首登改密链（初始密码→mustChangePassword→改密→旧密码失效+全族撤销）。
- 单元测试范围：错误码映射（AppError → 状态码 + `{code, title}`）、JWT claims 构造、分页/排序参数解析与 sort 白名单、初始密码生成器、argon2 hash/verify round-trip（默认参数，单次百 ms 级可接受）。
- 前端 locale key 对齐脚本（零依赖 node 脚本，`scripts/`）进 CI，直接守护 ADR 0002「两份 locale 同天提交」的纪律。
- backend job 顺序：`cargo fmt --check` → `cargo clippy --all-targets -D warnings` → `cargo sqlx migrate run` + `cargo sqlx prepare --check` → `cargo test`（PG service container）。
- `rust-toolchain.toml` 提交进仓库（≥1.94），版本即文档，CI 自动识别。
- sqlx-cli 安装**不带 `--locked`**：0.9 起仓库不提交 Cargo.lock（#8 调研标记的坑），`--features rustls` 避免 native-tls 系统依赖。
- CI PG 定 `postgres:18-alpine`（当前最新稳定大版本，EOL 2030）；与生产大版本的对齐留给部署事项（map fog），届时若生产选 17 需同步降 CI。
- 缓存：backend 用 Swatinem/rust-cache（key 含 `Cargo.lock` 与 `.sqlx/`），frontend 用 setup-node 内置 npm 缓存 + `npm ci`；不拆矩阵、不加 Docker 层缓存。
