# AgentChat Relay Worker

最小 Cloudflare Relay 骨架，当前只实现：

- `GET /v1/ws`
- relay token 鉴权
- `connection_id` 分配
- `relay_ready` 下发
- `from` 校验
- 按 `to` 路由转发 `secure_channel_hello` / `secure_channel_accept` / `relay_envelope`
- `PEER_OFFLINE` / `FORBIDDEN_SENDER` / `UNPAIRED_PEER` / `INVALID_SCHEMA` 级别的最小错误返回

当前**还没有**做：

- 正式 bootstrap / pairing 流程
- 真正端到端加密
- presence / peer_upsert / peer_remove
- 离线消息

## 本地开发

安装依赖：

```bash
cd relay
npm install
```

生成 Worker 类型：

```bash
npx wrangler types --outFile worker-configuration.d.ts
```

运行检查：

```bash
npm run typecheck
npm test
```

启动本地 Worker：

```bash
npm run dev
```

## 当前的开发辅助接口

为了先把 `/v1/ws`、鉴权和路由跑通，临时提供两个 **dev-only** 接口：

### `POST /v1/dev/bootstrap`

创建一个 daemon token，并初始化对应的 `DeviceHubDO`。

请求：

```json
{
  "device_id": "dev_local_1",
  "device_name": "local daemon"
}
```

响应：

```json
{
  "device_id": "dev_local_1",
  "relay_token": "achdm.dev_local_1.<secret>",
  "ws_url": "ws://127.0.0.1:8787/v1/ws"
}
```

### `POST /v1/dev/pair`

给指定 device 注册一个 app，并返回 app relay token。

请求：

```json
{
  "device_id": "dev_local_1",
  "app_installation_id": "app_local_1",
  "app_name": "local app"
}
```

响应：

```json
{
  "device_id": "dev_local_1",
  "app_installation_id": "app_local_1",
  "peer_id": "app:app_local_1",
  "relay_token": "achapp.dev_local_1.app_local_1.<secret>",
  "ws_url": "ws://127.0.0.1:8787/v1/ws"
}
```

## 手工验证思路

1. 调 `/v1/dev/bootstrap` 拿 daemon token
2. 调 `/v1/dev/pair` 拿 app token
3. 用两个 WebSocket 客户端分别连 `/v1/ws`
4. header 带上：`Authorization: Bearer <relay_token>`
5. 两端应先收到 `relay_ready`
6. app 发 `secure_channel_hello`
7. daemon 应收到原样转发的 frame
8. daemon 发 `secure_channel_accept`
9. app 应收到原样转发的 frame

## 项目结构

```text
relay/
├── package.json
├── wrangler.jsonc
├── tsconfig.json
├── README.md
└── src/
    ├── index.ts
    ├── device-hub.ts
    ├── pairing-index.ts
    ├── auth.ts
    ├── crypto.ts
    ├── protocol.ts
    └── types.ts
```
