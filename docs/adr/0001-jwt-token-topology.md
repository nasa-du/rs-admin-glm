# 认证令牌拓扑：access JWT + 不透明 refresh 轮换

管理后台需要可吊销的会话与 ≤15 min 的权限变更生效上界，而纯 JWT 无法吊销。决定：access 为 HS256 JWT（15 min，claims 携带单值 role，见 [#2](https://github.com/nasa-du/rs-admin-glm/issues/2) 角色层级），refresh 为 256-bit 不透明随机串（DB 只存 SHA-256 哈希），按 RFC 9700 轮换 + 重用检测 + 令牌族撤销，7 天滑动窗口；刷新时顺手重读 users 行，使禁用/角色变更在下次刷新时硬终止——上界由结构保证，不依赖各管理操作记得撤销。决议过程与完整参数见 [#3](https://github.com/nasa-du/rs-admin-glm/issues/3)，生态事实见 [#8](https://github.com/nasa-du/rs-admin-glm/issues/8)。

## Considered Options

- **access-only 长寿命 JWT（拒绝）**：零服务端状态、实现最简，但完全无法吊销——禁用一个恶意管理员后，他的 token 仍有效至过期（8–12h）。
- **refresh 做成 JWT（拒绝）**：轮换与重用检测反正要查库，行存在即有效；JWT 形态只多一套签名校验，毫无收益。
- **全局黑名单 / per-request 查库 / token 版本号（拒绝）**：即时吊销与 JWT 无状态不可兼得；已接受 ≤15 min 延迟。重读用户行只发生在刷新时（本来就要查库），是防纵深而非即时吊销——别把它"优化"成每请求校验。
- **access 存 localStorage / JS 可读 cookie（拒绝）**：XSS 可直接窃取；仅内存 + 路由守卫处启动刷新水合，代价只是硬刷新多一次往返。

## Consequences

- 「Postgres 哈希存储 + 轮换 + 重用检测」组合**无权威 Rust 范文**（[#8](https://github.com/nasa-du/rs-admin-glm/issues/8) 调研确认），属自实现工作量：一张表 + refresh 端点 + 既有 JWT 中间件。
- 密钥为单一 `JWT_SECRET` env（≥32 字节随机），不做 kid/JWKS 轮换；换密钥 = 全员下线，作为灾难恢复语义接受。
- 前端整体删除模板的 Zustand cookie persist 中间件（顺带消灭硬编码 key `'thisisjustarandomstring'`），refresh 走 `HttpOnly; Secure; SameSite=Strict; Path` 收窄的 cookie；CSRF 由 SameSite=Strict 加 refresh/logout 端点要求自定义 header 兜底。
- 登出 = 撤销当前浏览器对应的令牌族；改密码、被禁用 = 撤销该用户全部令牌族。
