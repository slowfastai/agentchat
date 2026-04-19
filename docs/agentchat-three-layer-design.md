# AgentChat 三层设计理念

## 1. 产品定位

AgentChat 首先是一款面向软件项目的项目管理工具，其核心价值不是把多个 agent 放进同一个聊天窗口，而是让人和 coding agent 能够围绕真实项目持续协作、推进事项、沉淀结果。

传统 coding agent 产品通常擅长完成一次 prompt 或一次会话，但不擅长管理一个项目的长期推进。随着任务增多，聊天记录、代码变更、分支、PR、测试结果、决策依据和后续事项会分散在不同位置。AgentChat 的设计目标是把这些内容重新组织回项目管理语境中。

因此，AgentChat 的顶层心智应是：

```text
Project 管长期上下文
Issue 管交付目标
Thread 管 agent 协作过程
```

换句话说，AgentChat 不是“带项目概念的聊天工具”，而是“以项目交付为核心的 agent 协作工具”。

## 2. 核心对象层级

AgentChat 采用三层产品结构：

```text
Project
  -> Issue
      -> Thread
```

这三个对象分别回答不同问题：

| 层级 | 核心问题 | 用户心智 |
| --- | --- | --- |
| Project | 我正在推进哪个项目？ | 长期工作空间 |
| Issue | 这个项目里现在要完成哪件事？ | 可管理的工作单元 |
| Thread | 为了完成这件事，agent 正在如何协作？ | 执行过程和对话现场 |

这个层级顺序非常关键。Issue 应该是产品中心对象，Thread 是推进 Issue 的协作机制。用户最终关心的是一件事是否完成，而不是某段聊天记录在哪。

## 3. Project：长期上下文和管理边界

Project 是 AgentChat 的顶层对象。早期产品中，Project 默认可以等同于一个 git repository，因为对开发者来说 repo 是最自然、最明确的项目边界。

Project 承载长期上下文，包括：

- 代码仓库或工作目录
- 项目内所有 Issues
- 项目级 agent 配置
- 常用命令、测试方式和构建方式
- 项目知识、约定和长期记忆
- 当前活跃 agent 工作
- 最近进展、分支、PR 和交付状态

Project 页面应该让用户一眼看到项目状态，而不是只看到聊天入口。它应回答：

- 当前有哪些事情在进行中？
- 哪些事项被 agent 接手了？
- 哪些事项阻塞了？
- 最近产生了哪些代码变更或决策？
- 哪些 Issue 需要用户介入？

### 3.1 Project 与 repo 的关系

第一版可以采用简单规则：

```text
一个 Project 默认绑定一个 primary repo
```

但数据模型不应把 Project 永久焊死为单一 repo。未来需要支持：

- 一个 Project 绑定多个 repositories
- 一个 Issue 横跨多个 repositories
- 一个 monorepo 内拆分多个 Projects
- 非代码型项目，例如文档、运营或设计项目

因此更稳妥的模型是：

```text
Project
  - primary_repository
  - optional linked_repositories[]
```

早期 UI 可以保持简单，但底层语义需要为更复杂的项目形态预留空间。

## 4. Issue：项目中的工作单元

Issue 是 AgentChat 的核心对象。它代表项目中一件需要推进的事，可以是 bug、feature、refactor、research、release、文档、设计讨论、CI 调查，或者任何有明确目标的项目事项。

Issue 的关键不是类型，而是目标：

```text
这件事完成以后，项目应该发生什么变化？
```

一个 Issue 应该承载：

- 标题和目标
- 背景说明
- 当前状态
- 优先级
- 负责人或参与者
- 相关文件、链接和外部 issue
- 相关分支、commit 和 PR
- 关联 Threads
- 测试结果和运行日志
- 决策记录
- 最终总结和交付物

Issue 让 agent 协作从“临时聊天”变成“可追踪的项目工作”。这也是 AgentChat 区别于普通 chat app 的关键。

### 4.1 Issue 的状态

Issue 状态应服务于项目管理心智，而不是服务于聊天状态。建议基础状态包括：

- Backlog
- Todo
- In Progress
- Blocked
- Review
- Done

Thread 或 agent session 的状态可以影响 Issue 状态，但不应完全等同于 Issue 状态。例如，一个 agent session completed 只说明某次执行结束，不代表 Issue 已完成。

### 4.2 Issue 页面应是工作控制台

Issue Detail 不应只是“描述 + 聊天列表”。它应该是一个围绕单个工作目标的控制台：

```text
Issue Detail
  - Goal
  - Context
  - Status
  - Threads
  - Artifacts
  - Decisions
  - Next Actions
```

其中 Artifacts 可以包括：

- branch
- commit
- PR
- test output
- screenshots
- generated files
- logs
- release notes

Decisions 可以记录：

- 选用了哪个方案
- 为什么没有选其他方案
- 哪些风险仍然存在
- 哪些后续事项需要拆成新 Issue

这能避免用户在长聊天里寻找结论。

## 5. Thread：围绕 Issue 的 agent 协作过程

Thread 是 Issue 下的执行和协作单元。它可以被理解为一次 agent group chat、一次 work session，或者一次围绕某个目标的执行现场。

一个 Issue 可以有一个 Thread，也可以有多个 Threads。多个 Threads 是必要设计，因为真实项目工作通常不是线性的：

- 一个 Thread 调研问题
- 一个 Thread 实现方案
- 一个 Thread 进行 review
- 一个 Thread 修复 CI
- 一个 Thread 补测试
- 一个 Thread 和用户讨论产品取舍
- 一个 Thread 整理结论并同步回 Issue

Thread 承载：

- 参与者，包括人类和 agents
- 对话记录
- agent thinking / tool call / plan / turn end
- 运行状态
- 关联 session
- 该次协作产生的结果

Thread 不应该抢走 Issue 的中心地位。用户可以进入 Thread 查看细节，但工作管理应回到 Issue。

### 5.1 Thread 与 Session 的关系

产品语义上，Thread 是用户可见的协作容器；Session 是某个 agent 的一次底层运行实例。

可以这样理解：

```text
Thread
  - human participants
  - agent participants
  - messages and timeline
  - sessions[]
```

一个 Thread 可以包含多个 agent sessions。例如，一个 group chat 中 Codex 和 Claude 同时参与，每个 agent 背后可以有自己的 session 生命周期。

这层区分很重要：

- Thread 面向用户，用于组织协作过程
- Session 面向系统，用于管理 agent 生命周期、重连、回放和事件日志

## 6. 边界规则

为了让模型稳定，建议采用以下判断规则。

### 6.1 什么时候创建 Project

当用户需要一个长期、独立的工作空间时，创建 Project。

典型情况：

- 一个 git repo
- 一个产品或 app
- 一个长期维护的代码库
- 一个独立客户项目
- 一个实验项目

### 6.2 什么时候创建 Issue

当用户能描述一个需要完成的事项时，创建 Issue。

典型情况：

- 修复一个 bug
- 开发一个功能
- 调查一个异常
- 重构一个模块
- 准备一次发布
- 撰写或更新文档
- 做一次技术方案调研

判断标准：

```text
如果用户关心“这件事是否完成”，它应该是 Issue。
```

### 6.3 什么时候创建 Thread

当用户需要围绕某个 Issue 发起一次 agent 协作时，创建 Thread。

典型情况：

- 让一个 agent 调研方案
- 让一个 agent 实现代码
- 让另一个 agent review
- 让多个 agents 并行讨论
- 在同一 Issue 下切换到新的执行方向

判断标准：

```text
如果用户关心“agent 是如何讨论、执行和产出结果的”，它应该是 Thread。
```

## 7. 典型工作流

### 7.1 单 agent 修 bug

```text
Project: AgentChat
  Issue: Fix WebSocket reconnect failure
    Thread: Investigate reconnect logs
      Agent: Codex
    Thread: Implement reconnect fix
      Agent: Codex
    Thread: Review edge cases
      Agent: Claude
```

Issue 是用户管理的对象，三个 Threads 是推进该 Issue 的不同阶段。

### 7.2 多 agent 开发功能

```text
Project: AgentChat
  Issue: Add project issue dashboard
    Thread: Product scope discussion
      Human + Product Agent
    Thread: UI implementation
      Codex
    Thread: Backend protocol changes
      Codex + Claude
    Thread: QA and regression review
      Review Agent
```

这里 multi-agent 的价值不是“聊天热闹”，而是把不同能力的 agent 组织到同一个 Issue 的不同执行现场中。

### 7.3 调研后拆分新事项

```text
Project: AgentChat
  Issue: Investigate slow daemon startup
    Thread: Profile startup path
      Agent: Codex
    Decisions:
      - Startup delay mainly comes from agent initialization.
      - Caching config can be fixed immediately.
      - Lazy agent startup should become a separate Issue.

  Issue: Add lazy agent startup
```

Issue 可以产生新的 Issue，形成项目推进链路。

## 8. Multi-agent 的产品价值

AgentChat 的 multi-agent 不应只是“多个机器人在一个聊天窗口里说话”。真正价值是角色化、并行化和可审计。

不同 agent 可以承担不同角色：

- Researcher：查文档、读代码、总结背景
- Implementer：修改代码、运行测试、提交 patch
- Reviewer：检查风险、找边界条件、审查实现
- Debugger：看日志、定位失败、修 CI
- Product Partner：澄清需求、比较方案、整理取舍
- Summarizer：把 Thread 结论同步回 Issue

这让 Thread 成为可组织的执行现场，而不是普通聊天记录。

## 9. 命名建议

底层模型可以继续使用 `Thread`，因为它适合表示一组有序事件和消息。

但用户侧文案可以更灵活：

| 场景 | 建议文案 |
| --- | --- |
| 单个执行现场 | Work Session |
| 多 agent 对话 | Group Chat |
| Issue 下的会话列表 | Agent Chats |
| 技术数据模型 | Thread |

推荐原则：

```text
数据模型可以叫 Thread
用户界面可以叫 Agent Chat 或 Work Session
```

这样既保留工程表达，也避免用户把产品理解为普通聊天工具。

## 10. UI 信息架构建议

第一版可以围绕以下页面组织：

```text
Projects
  -> Project Dashboard
      -> Issues
          -> Issue Detail
              -> Threads
              -> Artifacts
              -> Decisions
```

### 10.1 Project Dashboard

Project Dashboard 应突出项目管理：

- Open Issues
- Running Threads
- Blocked Issues
- Needs Review
- Recent Agent Activity
- Recent PRs / Branches

### 10.2 Issue Detail

Issue Detail 是核心工作台：

- Issue goal
- Current status
- Context
- Thread list
- Active thread
- Artifacts
- Decisions
- Next actions

### 10.3 Thread View

Thread View 是执行细节：

- Timeline
- Messages
- Agent thinking
- Tool calls
- Plans
- Session status
- Result summary

Thread View 的输出应该能同步回 Issue，例如生成 summary、decision、artifact 或 next action。

## 11. 数据模型提示

一个可扩展的数据模型可以从以下关系开始：

```text
Project
  id
  name
  primary_repo_path
  linked_repositories[]
  issues[]

Issue
  id
  project_id
  number
  title
  summary
  status
  priority
  participants[]
  threads[]
  artifacts[]
  decisions[]

Thread
  id
  issue_id
  title
  participants[]
  state
  timeline[]
  sessions[]

Session
  id
  thread_id
  agent_id
  state
  event_log
```

这里的关键是：`Issue` 不依赖单一 `Thread`，`Thread` 不等同于单一 `Session`。这样才能支持多 agent、重连、回放、并行协作和多阶段工作流。

## 12. 产品原则

AgentChat 的三层设计应遵循以下原则：

1. Project 是长期边界，不是临时聊天分组。
2. Issue 是中心对象，承载目标、状态和交付结果。
3. Thread 是执行过程，服务于 Issue。
4. Multi-agent 应服务于项目推进，而不是成为聊天噱头。
5. 所有重要结论都应该从 Thread 回流到 Issue。
6. 用户应该能从 Project 层看到整体进展，从 Issue 层判断完成度，从 Thread 层追溯执行细节。
7. Session 是底层运行机制，不应替代用户可理解的 Thread。

## 13. 一句话总结

AgentChat 的三层设计可以概括为：

```text
每个项目都有要推进的事；
每件事都可以组织 agent 协作；
每次协作都沉淀为可追踪的进展。
```

这就是 `Project -> Issue -> Thread` 的产品基础，也是 AgentChat 相比普通 coding agent chat app 的核心差异。
