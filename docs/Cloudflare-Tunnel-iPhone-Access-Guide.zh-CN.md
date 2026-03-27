# AgentChat 通过 Cloudflare Tunnel 让 iPhone App 公网访问 daemon（中文指南）

> 适用仓库：`agentchat`
> 最后更新：2026-03-27
> 相关代码：`daemon/server/src/ws.rs`、`daemon/bin/src/main.rs`、`daemon/PROTOCOL.md`

---

## 一、目标

本文档说明如何在**不使用 VPS、不中转到 relay Worker** 的前提下，使用 **Cloudflare Tunnel** 把本机运行的 `agentchat-daemon` 暴露成一个公网 `wss://` 地址，供 iPhone App 扫码连接。

适合场景：

- 自己使用或小范围测试
- 快速验证 iPhone App 能否跨网络连上 Mac 上的 daemon
- 不想先搭建 VPS 或完善 relay 基础设施

不太适合的场景：

- 面向大量陌生用户的正式公网服务
- 需要严格鉴权、限流、审计日志的生产环境
- 需要一个长期稳定、可运维、可扩展的公网入口层

---

## 二、先说结论

当前仓库里，daemon 的**直连模式**本身就是一个 WebSocket 服务，因此最省事的办法是：

```text
iPhone App
  -> wss://你的公网域名
  -> Cloudflare Tunnel
  -> Mac 上的 agentchat-daemon:9390
```

有两个关键点一定要注意：

1. **直连 daemon 时，URL 用根路径即可**

   例如：

   ```text
   wss://claudecodes.top
   ```

   不要写成：

   ```text
   wss://claudecodes.top/v1/ws
   ```

2. **`/v1/ws` 是 relay Worker 的路径，不是 direct daemon 的路径**

也就是说，本文档讲的是：

- **Cloudflare Tunnel -> 直接暴露 daemon 的 WebSocket 服务**
- **不是** 仓库里的 `relay/` 方案

---

## 三、前置条件

你需要准备：

- 一台运行本仓库的 Mac
- 已安装 Rust / Cargo
- 已安装 `cloudflared`
- 若使用稳定域名方案，还需要：
  - 一个 Cloudflare 账号
  - 一个已经托管到 Cloudflare 的域名

安装 `cloudflared`：

```bash
brew install cloudflared
```

### 补充：`cloudflared` 和 `wrangler` 的区别

这两个工具很容易混淆，但用途并不一样：

- `cloudflared`：Cloudflare Tunnel 客户端，用来把你本机的 `localhost` 服务暴露成公网可访问地址
- `wrangler`：Cloudflare Workers 的开发 / 部署 CLI，用来部署 Worker、Durable Objects、KV 等云端代码

对本仓库来说：

- 如果你要做的是 **“把 Mac 上的 direct daemon 暴露给 iPhone App”**，用的是 **`cloudflared`**
- 如果你要部署的是仓库里的 **`relay/` Cloudflare Worker**，用的是 **`wrangler`**

本文档只覆盖 **`cloudflared + direct daemon`** 这一条路径。

---

## 四、方案 A：先用 TryCloudflare 临时地址快速验证

这是最快的 0→1 验证路径。

### 4.1 编译 daemon 和测试用 fake agent

在仓库根目录执行：

```bash
cd /Users/a1-6/Downloads/agentchat

cargo build --manifest-path daemon/Cargo.toml \
  -p agentchat-daemon --bin agentchat-daemon

cargo build --manifest-path daemon/Cargo.toml \
  -p agentchat-server --bin fake_acp_agent
```

编译产物默认会在：

```text
/Users/a1-6/Downloads/agentchat/daemon/target/debug/agentchat-daemon
/Users/a1-6/Downloads/agentchat/daemon/target/debug/fake_acp_agent
```

---

### 4.2 启动临时 Cloudflare Tunnel

打开一个新终端（终端 A）：

```bash
cloudflared tunnel --url http://127.0.0.1:9390
```

它会输出一个临时地址，例如：

```text
https://random-name.trycloudflare.com
```

对应给 iPhone App 使用的 WebSocket 地址就是：

```text
wss://random-name.trycloudflare.com
```

> 说明：临时地址每次启动都可能变化，所以每次变更后都要重新生成二维码。

请保持这个终端持续运行。

---

### 4.3 启动 daemon，并把公网地址写进二维码

打开另一个终端（终端 B）：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_AGENT_ID=fake \
AGENTCHAT_AGENT_NAME="Fake ACP Agent" \
AGENTCHAT_AGENT_COMMAND="$PWD/daemon/target/debug/fake_acp_agent" \
AGENTCHAT_MOBILE_WS_URL="wss://random-name.trycloudflare.com" \
cargo run --manifest-path daemon/Cargo.toml \
  -p agentchat-daemon --bin agentchat-daemon -- --mobile
```

把上面的 `wss://random-name.trycloudflare.com` 替换成你实际拿到的临时地址。

这个命令会做两件事：

1. 启动 daemon（默认监听 `:9390`）
2. 在终端里打印一个二维码，二维码内容就是公网 `wss://...` 地址

---

### 4.4 在 iPhone App 中扫码连接

在 iPhone App 中进入：

```text
Connection → Scan QR
```

扫描终端 B 输出的二维码。

如果连接成功，通常会看到：

- `cloudflared` 终端里出现代理流量
- daemon 日志里出现新的 WebSocket 连接
- iPhone App 可以创建会话并收到 fake agent 的流式响应

---

### 4.5 本地先做一个 smoke test（可选但推荐）

如果你想先验证 daemon 本地是否正常，可以在第三个终端里执行：

```bash
cd /Users/a1-6/Downloads/agentchat/daemon
python3 scripts/ws_smoke_test.py
```

这个脚本验证的是本地直连：

```text
ws://127.0.0.1:9390
```

如果本地 smoke test 成功，而 iPhone 连不上，那么问题通常出在：

- Tunnel 没启动
- 公网 URL 填错
- 终端 A 已退出
- 扫码内容不是最新地址

---

## 五、方案 B：使用命名 Tunnel + 自有域名（稳定地址）

如果你不想每次都使用 `trycloudflare.com` 的临时地址，可以创建一个命名 Tunnel，并绑定稳定域名。本文档后续统一使用你的真实域名：

```text
wss://claudecodes.top
```

---

### 5.1 登录 Cloudflare

```bash
cloudflared tunnel login
```

命令会打开浏览器，让你选择授权的域名。

---

### 5.2 创建命名 Tunnel

```bash
cloudflared tunnel create agentchat
```

创建成功后会输出一个 **Tunnel ID**，并在本机生成凭据文件，通常位于：

```text
~/.cloudflared/<TUNNEL_ID>.json
```

记住这个 `TUNNEL_ID`。

> 如果你在下一步执行 `cloudflared tunnel route dns agentchat ...` 时看到：
>
> ```text
> agentchat is neither the ID nor the name of any of your tunnels
> ```
>
> 通常表示：
>
> - 你还没有真正创建成功 named tunnel
> - 或者当前 `cloudflared` 登录的是另一个 Cloudflare 账号 / zone 上下文
>
> 可先执行：
>
> ```bash
> cloudflared tunnel list
> ```
>
> 如果输出里没有 `agentchat`，重新执行：
>
> ```bash
> cloudflared tunnel create agentchat
> ```
>
> 如果 `cloudflared tunnel list` 直接显示：
>
> ```text
> No tunnels were found for the given filter flags.
> ```
>
> 也说明当前账号下还没有任何 named tunnel。

---

### 5.3 把域名路由到 Tunnel

本文档直接使用你的真实域名：

```text
claudecodes.top
```

执行：

```bash
cloudflared tunnel route dns agentchat claudecodes.top
```

这会在 Cloudflare 上创建对应的 DNS 路由。

#### 5.3.1 针对 `claudecodes.top` 的可直接复制示例

```bash
cloudflared tunnel login
cloudflared tunnel create agentchat
cloudflared tunnel list
cloudflared tunnel route dns agentchat claudecodes.top
```

如果 `cloudflared tunnel list` 的输出里没有 `agentchat`，就不要直接执行 `route dns`，而是先把 `create` 这一步做成功。

后续二维码里的公网地址也应写成：

```text
wss://claudecodes.top
```

> 如果你后续希望把 Tunnel 和网站首页拆开，再改成子域名（例如 `agent.claudecodes.top`）也可以；但本文档当前默认以 `claudecodes.top` 为准。

---

### 5.4 创建 `cloudflared` 配置文件

创建文件：

```text
~/.cloudflared/config.yml
```

内容如下：

```yaml
tunnel: TUNNEL_ID_HERE
credentials-file: /Users/a1-6/.cloudflared/TUNNEL_ID_HERE.json

ingress:
  - hostname: claudecodes.top
    service: http://127.0.0.1:9390
  - service: http_status:404
```

请替换：

- `TUNNEL_ID_HERE`
- 如果你的本机用户名不是 `a1-6`，也要同步修改 `credentials-file` 路径

> 说明：虽然这里写的是 `http://127.0.0.1:9390`，但 Cloudflare Tunnel 会正确代理 WebSocket Upgrade，请求最终仍会到达 daemon 的 WebSocket 服务。
>
> 另外，`config.yml` 里的 `tunnel:` 字段应该填写 **Tunnel ID（UUID）**，不是 tunnel 名字 `agentchat`。

#### 5.4.1 `claudecodes.top` 对应的 `~/.cloudflared/config.yml` 完整最终版

```yaml
# ~/.cloudflared/config.yml
# 注意：下面的 TUNNEL_ID 要替换成 `cloudflared tunnel create agentchat` 输出的真实 UUID

tunnel: TUNNEL_ID_HERE
credentials-file: /Users/a1-6/.cloudflared/TUNNEL_ID_HERE.json

ingress:
  - hostname: claudecodes.top
    service: http://127.0.0.1:9390
  - service: http_status:404
```

如果你创建 tunnel 之后得到的 UUID 是 `12345678-1234-1234-1234-123456789abc`，那么最终文件就应该长这样：

```yaml
tunnel: 12345678-1234-1234-1234-123456789abc
credentials-file: /Users/a1-6/.cloudflared/12345678-1234-1234-1234-123456789abc.json

ingress:
  - hostname: claudecodes.top
    service: http://127.0.0.1:9390
  - service: http_status:404
```

你可以用下面的命令快速打开编辑：

```bash
mkdir -p ~/.cloudflared
open -a TextEdit ~/.cloudflared/config.yml
```

或使用命令行编辑器：

```bash
nano ~/.cloudflared/config.yml
```

---

### 5.5 启动命名 Tunnel

打开终端 A：

```bash
cloudflared tunnel run agentchat
```

保持该进程运行。

---

### 5.6 启动 daemon，并将稳定域名写入二维码

打开终端 B：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_AGENT_ID=fake \
AGENTCHAT_AGENT_NAME="Fake ACP Agent" \
AGENTCHAT_AGENT_COMMAND="$PWD/daemon/target/debug/fake_acp_agent" \
AGENTCHAT_MOBILE_WS_URL="wss://claudecodes.top" \
cargo run --manifest-path daemon/Cargo.toml \
  -p agentchat-daemon --bin agentchat-daemon -- --mobile
```

这样二维码里编码的就是稳定地址：

```text
wss://claudecodes.top
```

---

### 5.7 在 iPhone App 中扫码

进入：

```text
Connection → Scan QR
```

扫描二维码即可。

如果一切正常，iPhone 即使不在同一局域网，也能连接到 Mac 上的 daemon。

---

## 六、从 fake agent 切换到 OpenCode + Codex

当 Tunnel 和二维码链路已经验证成功后，就可以把测试用的 `fake_acp_agent` 换成真实 agent 组合。

推荐目标是：

- OpenCode
- Codex

如果你想先单独验证 OpenCode，也可以先做一个过渡步骤。

### 6.1 只使用 OpenCode（可选过渡步骤）

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_AGENT_ID=opencode \
AGENTCHAT_AGENT_NAME="OpenCode (ACP)" \
AGENTCHAT_AGENT_COMMAND=opencode \
AGENTCHAT_AGENT_ARGS="acp" \
AGENTCHAT_MOBILE_WS_URL="wss://claudecodes.top" \
cargo run --manifest-path daemon/Cargo.toml \
  -p agentchat-daemon --bin agentchat-daemon -- --mobile
```

### 6.2 同时使用 OpenCode + Codex（推荐）

本仓库已经提供了一个现成脚本：

- `daemon/scripts/run_daemon_both.sh`

它会自动构造 `AGENTCHAT_AGENTS_JSON`，并启动两个真实 agent：

- OpenCode（backend: `acp`）
- Codex（backend: `codex_app_server`）

先确认你的本机上这两个命令都可用：

```bash
opencode --help
codex --help
```

然后执行：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_MOBILE_WS_URL="wss://claudecodes.top" \
bash daemon/scripts/run_daemon_both.sh --mobile
```

如果你需要自定义名字、ID 或工作目录，也可以这样：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_MOBILE_WS_URL="wss://claudecodes.top" \
AGENTCHAT_BOTH_WORKING_DIR="/Users/a1-6/your/project" \
AGENTCHAT_OPENCODE_ID="opencode-main" \
AGENTCHAT_OPENCODE_NAME="OpenCode" \
AGENTCHAT_CODEX_ID="codex-main" \
AGENTCHAT_CODEX_NAME="Codex" \
bash daemon/scripts/run_daemon_both.sh --mobile
```

这样 iPhone App 扫码后，应当能看到两个可选 agent：

- OpenCode
- Codex

如果你有更复杂的多 agent 配置，也可以直接使用：

- `AGENTCHAT_AGENTS_JSON`

来自定义所有 agent。

---

## 七、常见问题排查

### 7.1 二维码扫了，但 iPhone 连不上

优先检查：

1. `cloudflared` 是否还在运行
2. `AGENTCHAT_MOBILE_WS_URL` 是否是最新值
3. 是否错误地用了 `/v1/ws`
4. Mac 是否休眠
5. daemon 是否真的监听在 `9390`

---

### 7.2 路径到底要不要写 `/v1/ws`

**不要。**

对于本文档的 direct daemon + Cloudflare Tunnel 方案，应该使用：

```text
wss://claudecodes.top
```

而不是：

```text
wss://claudecodes.top/v1/ws
```

因为：

- `wss://host`：对应本仓库的 direct WebSocket daemon
- `wss://host/v1/ws`：对应本仓库 `relay/` 里的 relay Worker

两者不是同一个入口。

---

### 7.3 执行 `cloudflared tunnel route dns ...` 时提示找不到 tunnel

如果你看到错误：

```text
agentchat is neither the ID nor the name of any of your tunnels
```

通常说明：

- `agentchat` 这个 named tunnel 还没有创建
- 或者你当前登录到的不是创建该 tunnel 的那个 Cloudflare 账号

建议按下面顺序排查：

```bash
cloudflared tunnel list
```

如果结果是：

```text
No tunnels were found for the given filter flags.
```

说明当前上下文下根本没有 named tunnel，需要先创建：

```bash
cloudflared tunnel create agentchat
```

然后再执行：

```bash
cloudflared tunnel route dns agentchat claudecodes.top
```

---

### 7.4 本地 smoke test 能过，但公网还是不通

通常说明 daemon 本身是好的，问题更可能在公网入口层：

- Tunnel 没有把流量正确转发到 `127.0.0.1:9390`
- 域名还没正确生效
- 你使用了旧二维码
- `cloudflared` 进程已退出或配置错误

可以优先看两处日志：

1. `cloudflared` 终端输出
2. daemon 终端输出

---

### 7.5 为什么建议先用 fake agent

因为这样可以先把问题拆开：

- 第一层：iPhone ↔ Cloudflare Tunnel ↔ daemon 的网络链路是否通
- 第二层：真实 agent 后端是否工作正常

如果一上来就用真实 agent，遇到问题时很难分清是：

- Tunnel 问题
- daemon 问题
- 还是 agent 进程问题

---

## 八、安全与风险提醒

这个方案的优点是快，但代价是：**你暴露的是 daemon 的原生 WebSocket 接口。**

因此它更适合：

- 自己使用
- 少量测试设备
- MVP 验证

不建议直接作为正式公网产品入口，原因包括：

- 当前不是专门的公网 API Gateway
- 没有额外的接入鉴权层
- 没有完整的公网限流 / 审计 / 风控策略

如果后续要正式对外提供服务，更推荐演进到下面两类方案之一：

1. **VPS + Caddy/Nginx + Tailscale**
   - 更适合长连接、流式输出、可控性更高
2. **完善仓库自带的 relay 方案**
   - 即 `relay/` + `daemon/server/src/relay.rs`
   - 但当前仍偏开发态，尚未完全产品化

---

## 九、推荐实践顺序

建议按下面的顺序落地：

### 第一步：TryCloudflare 临时地址

目标：验证公网直连是否可行。

### 第二步：命名 Tunnel + 稳定域名

目标：把测试环境变成稳定可复用的私人入口。

### 第三步：fake agent → OpenCode / Codex

目标：只替换业务后端，不改公网接入方式。

### 第四步：需要正式化时再升级架构

如果开始有更多用户、更多设备、更复杂的鉴权需求，再迁移到：

- VPS 前置网关
- 或仓库内的 relay 体系

---

## 十、最小命令清单（便于复制）

### 10.1 临时 Tunnel

```bash
cd /Users/a1-6/Downloads/agentchat

cargo build --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon
cargo build --manifest-path daemon/Cargo.toml -p agentchat-server --bin fake_acp_agent

cloudflared tunnel --url http://127.0.0.1:9390
```

另开终端：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_AGENT_ID=fake \
AGENTCHAT_AGENT_NAME="Fake ACP Agent" \
AGENTCHAT_AGENT_COMMAND="$PWD/daemon/target/debug/fake_acp_agent" \
AGENTCHAT_MOBILE_WS_URL="wss://你的-trycloudflare-域名" \
cargo run --manifest-path daemon/Cargo.toml \
  -p agentchat-daemon --bin agentchat-daemon -- --mobile
```

### 10.2 稳定域名 Tunnel

> 本文档默认使用你的真实域名 `claudecodes.top`。

```bash
cloudflared tunnel login
cloudflared tunnel create agentchat
cloudflared tunnel list
cloudflared tunnel route dns agentchat claudecodes.top
cloudflared tunnel run agentchat
```

另开终端：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_AGENT_ID=fake \
AGENTCHAT_AGENT_NAME="Fake ACP Agent" \
AGENTCHAT_AGENT_COMMAND="$PWD/daemon/target/debug/fake_acp_agent" \
AGENTCHAT_MOBILE_WS_URL="wss://claudecodes.top" \
cargo run --manifest-path daemon/Cargo.toml \
  -p agentchat-daemon --bin agentchat-daemon -- --mobile
```

### 10.3 稳定域名 + OpenCode + Codex

先确认本机已安装：

```bash
opencode --help
codex --help
```

然后在 Tunnel 已经运行的前提下，另开终端执行：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_MOBILE_WS_URL="wss://claudecodes.top" \
bash daemon/scripts/run_daemon_both.sh --mobile
```

如果你想指定实际工作目录：

```bash
cd /Users/a1-6/Downloads/agentchat

AGENTCHAT_MOBILE_WS_URL="wss://claudecodes.top" \
AGENTCHAT_BOTH_WORKING_DIR="/Users/a1-6/your/project" \
bash daemon/scripts/run_daemon_both.sh --mobile
```

---

## 十一、相关文件索引

与本文档相关的仓库文件：

- daemon WebSocket 服务：`daemon/server/src/ws.rs`
- 二维码与移动端 URL 覆盖：`daemon/bin/src/main.rs`
- WebSocket 协议说明：`daemon/PROTOCOL.md`
- relay 方案说明：`daemon/RELAY.md`
- relay Worker：`relay/`

---

如果后续需要，可以在此文档基础上继续补充：

- Cloudflare Access 前置鉴权
- named tunnel 作为系统服务自启动
- 生产环境如何从 direct tunnel 迁移到 VPS / relay 架构
