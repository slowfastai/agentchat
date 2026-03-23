# Relay Smoke Clients

当前仓库里有三块最小 relay 验证资产：

- daemon-side Rust smoke client
- app-side Rust smoke client
- 一键本地 E2E Python smoke script

它们一起用于验证：

- 连接 `/v1/ws`
- 收到 `relay_ready`
- app 发 `secure_channel_hello`
- daemon 回 `secure_channel_accept`
- 使用真实 `Ed25519` 签名 hello / accept
- 使用真实 `X25519 + HKDF-SHA256` 派生会话密钥
- 双方计算出同一个 `channel_id`
- app -> daemon 发送真实加密 `relay_envelope`
- daemon -> app 返回真实加密 `relay_envelope`
- replay 的 envelope 被本地 `seq` 保护拒绝

## 最推荐：一键本地 E2E

先启动本地 relay Worker：

```bash
cd relay
npm run dev
```

然后在另一个终端运行：

```bash
cd daemon
python3 scripts/relay_smoke_e2e.py
```

这个脚本会自动：

1. 调用 relay 的 dev bootstrap / pair 接口
2. 启动 `relay_smoke_daemon`
3. 启动 `relay_smoke_app`
4. 等待双方完成真实签名握手
5. 校验双方打印了同一个 `channel_id`
6. 校验双方都报告 `has_session_keys=true`
7. 校验 app -> daemon 的真实密文 envelope 能被解密
8. 校验 daemon -> app 的真实密文 envelope 能被解密
9. 校验 replay protection 触发 `SEQ_REPLAY`

## 手工运行 daemon smoke client

```bash
cd daemon
AGENTCHAT_RELAY_WS_URL='ws://127.0.0.1:8787/v1/ws' \
AGENTCHAT_RELAY_TOKEN='achdm.dev_local_1.<secret>' \
cargo run -p agentchat-daemon --bin relay_smoke_daemon
```

daemon 侧会：

1. 连接 relay
2. 打印 `relay_ready`
3. 校验 app 的 `secure_channel_hello` 签名
4. 自动回真实签名的 `secure_channel_accept`
5. 派生会话密钥并打印 `channel_id`

## 手工运行 app smoke client

```bash
cd daemon
AGENTCHAT_RELAY_WS_URL='ws://127.0.0.1:8787/v1/ws' \
AGENTCHAT_RELAY_TOKEN='achapp.dev_local_1.app_local_1.<secret>' \
cargo run -p agentchat-daemon --bin relay_smoke_app
```

app 侧会：

1. 连接 relay
2. 收到 `relay_ready`
3. 自动发送真实签名的 `secure_channel_hello`
4. 校验 `secure_channel_accept` 签名
5. 派生会话密钥并打印 `channel_id`

## 实现位置

- 协议辅助：`daemon/protocol/src/relay.rs`
- relay crypto：`daemon/protocol/src/relay_crypto.rs`
- crypto fixture：`daemon/protocol/fixtures/relay/crypto/handshake_v1.json`
- fixture 生成器：`daemon/bin/src/bin/relay_crypto_fixture.rs`
- Rust relay client：`daemon/core/src/relay_client.rs`
- relay integration tests：`daemon/core/tests/relay_integration.rs`
- daemon smoke binary：`daemon/bin/src/bin/relay_smoke_daemon.rs`
- app smoke binary：`daemon/bin/src/bin/relay_smoke_app.rs`
- 一键 E2E 脚本：`daemon/scripts/relay_smoke_e2e.py`
