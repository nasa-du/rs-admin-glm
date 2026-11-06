# API 契约与前端接线：`/api` 前缀、`{code, title}` 错误体、refresh 携带 user

前后端唯一的集成面是一组 REST 端点：全部挂 `/api` 前缀（开发期 Vite proxy 一条规则转发，生产同源、不配 CORS，见 [#5](https://github.com/nasa-du/rs-admin-glm/issues/5)），JSON 一律 camelCase。错误体定为 `{code, title}`：`code` 是稳定语义错误码（[#4](https://github.com/nasa-du/rs-admin-glm/issues/4) 决议），`title` 是英文兜底文案——模板已有的 `error.response?.data.title` 消费点零改动可用，前端按 code 映射界面语言、无映射回退 title；422 表单错误附 `errors: {field: [code]}`。refresh 响应直接携带 `{accessToken, user}`（刷新时本就重读用户行，见 [#3](https://github.com/nasa-du/rs-admin-glm/issues/3)），故不设独立的 `GET /auth/me`。决议过程与完整端点清单见 [#7](https://github.com/nasa-du/rs-admin-glm/issues/7)。

## Considered Options

- **无前缀根挂载（拒绝）**：前端页面路由 `/users` 与 API `/users` 同源冲突，Vite proxy 需逐路径枚举，SPA 回退与 API 分流纠缠。
- **错误体只出 code 或只出 title（拒绝）**：只出 code 则模板 toast 直接读 `data.title` 的现有消费点全要改，且未映射 code 会裸奔给用户；只出 title 违背语义码过边界的 i18n 决议。双字段是两者的调和。
- **保留 `GET /auth/me`（拒绝）**：REST 习惯更足，但守卫水合要 refresh + me 两次串行往返；refresh 端点本就重读用户行（[#3](https://github.com/nasa-du/rs-admin-glm/issues/3)），顺手返回零成本。
- **改密保留当前会话（拒绝）**：要求后端实现「撤销除当前族外全部」的特例，与「改密 = 撤销全部」的 [#3] 决议原文分叉；改完重新登录的体验代价可接受（首登强制改密走同一条路）。
- **宽限窗缓解多标签页刷新竞态（拒绝）**：后端多存一个前代哈希、泄露判定出现 ~30s 盲区；`navigator.locks` 在前端一处串行化即可，后端保持纯轮换。
- **401 处理留在 queryCache 全局钩子（拒绝）**：模板原方案 401 即登出，没有「先试刷新再重放」的机会；搬到 axios 响应拦截器（单飞 refresh + 并发排队 + 重放一次）后，全局钩子只留 500 分支。

## Consequences

- axios 拦截器的 refresh 调用须包在 `navigator.locks.request('auth-refresh', …)` 里跨标签页串行，否则两个标签页同时过期会拿同一个 cookie 并发刷新，一个成功一个被重用检测判死，整族撤销、两边全下线。
- `main.tsx` 中 queryCache.onError 的 401 分支删除（由拦截器接管）；mutation toast 与 500→/500 分支保留。
- 新增 `POST /api/users/:id/password-reset`（向下管理，返回一次性初始密码、置回 mustChangePassword、撤销该用户全部令牌族）与 `PATCH /api/users/me`（仅自我显示名）；`role` 字段仅超管可改，`isEnabled` 不可作用于自己。
- 列表契约：`page / pageSize / sort / role（可重复）/ status（单值）/ q`，响应 `{data, meta: {total, page, pageSize}}`；模板的 username 列过滤与 4 值 status 随字段裁剪一并移除。
- 改密的「当前密码错误」不共用 401（会触发无谓的刷新重放），定为 422 字段级错误。
- 登录页 zod 密码下限 7→8，对齐 [#6](https://github.com/nasa-du/rs-admin-glm/issues/6) 的 8–128。
