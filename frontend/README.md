# rs-admin frontend

rs-admin-glm 管理后台前端。基于 [satnaing/shadcn-admin](https://github.com/satnaing/shadcn-admin)
v2.2.1（MIT License，见 [LICENSE](./LICENSE)）裁剪：仅保留 dashboard / users / sign-in / settings
四组页面与 403 / 500 错误页，zh-CN / en 双语（i18next，见 `src/locales/`）。

## 开发

```bash
npm ci            # 或 npm install
npm run dev       # http://localhost:5173，/api 代理到 http://localhost:3000
```

## 门禁（与 CI frontend workflow 一致）

```bash
npm run lint            # eslint
npm run build           # tsc -b && vite build
npm run locales:check   # 两份 locale 的 key 集合必须一致
npx knip                # 未使用文件/依赖检查（本地）
```

新增界面文案：先加 key 到 `src/locales/zh-CN.json` 与 `en.json`（缺一份则 CI 红），
组件内不写裸字符串（ADR 0002）。

上游模板的完整文档：<https://shadcn-admin.netlify.app/>
