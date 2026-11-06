# rs-admin frontend

rs-admin-glm 管理后台前端。基于 [satnaing/shadcn-admin](https://github.com/satnaing/shadcn-admin)
v2.2.1（MIT License，见 [LICENSE](./LICENSE)）裁剪：仅保留 dashboard / users / sign-in / settings
四组页面与 403 / 500 错误页。

当前处于 M0 基座阶段：登录为模板的 mock 实现（任意邮箱 + 密码即可进入 dashboard），
真实认证（JWT + refresh 轮换）在后续里程碑接入。

## 开发

```bash
npm install
npm run dev        # http://localhost:5173，/api 代理到 http://localhost:3000
```

门禁（与 CI 一致）：

```bash
npm run lint       # eslint
npm run build      # tsc -b && vite build
npx knip           # 未使用文件/依赖检查
```

上游模板的完整文档：<https://shadcn-admin.netlify.app/>
