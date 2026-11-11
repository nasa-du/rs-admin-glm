# rs-admin frontend

shadcn-admin v2.2.1 fork 裁剪版：dashboard / users / sign-in / settings 四组页面 +
403 / 500 错误页，zh-CN / en 双语（i18next，见 `src/locales/`）。

## 快速开始

```bash
npm ci          # 或 npm install
npm run dev     # http://localhost:5173，/api 代理到 127.0.0.1:3000（先起后端）
```

## 门禁（与 CI frontend workflow 一致）

```bash
npm run lint            # eslint
npm run build           # tsc -b + vite build
npm run locales:check   # 两份 locale 的 key 集合必须一致（新增文案同天改两份）
```

新增界面文案：先加 key 到 `src/locales/zh-CN.json` 与 `en.json`（缺一份则 CI 红），
组件内不写裸字符串（ADR 0002）。
