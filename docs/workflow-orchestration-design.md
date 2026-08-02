# AgentChat 工作流编排设计

## 1. 要解决的问题

现在的多 agent 开发流程是人肉编排的：一个模型写 plan，其余模型分别审核，人工把审核意见复制回给写 plan 的模型，反复若干轮；plan 定稿后找一个模型实现，再把代码交给其余模型审核，又是反复若干轮。一次需求要花一天到几天，而且全程需要人在场——不是在思考，是在等某个 agent 输出、然后搬运文本。

真正的瓶颈不是"agent 不够多"，而是：

- 阶段推进依赖人工搬运，人成了消息总线
- 讨论没有终止条件，靠人判断"差不多了"
- 过程不可恢复，关掉终端就得重来
- 人被迫参与每一步，而实际上只有两个环节需要人的判断

因此 AgentChat 需要的不是"更大的群聊"，而是一条**可编排、可审计、可暂停恢复的流水线**，人只保留关键决策点。

## 2. 与三层设计的关系

`docs/agentchat-three-layer-design.md` 定义了 `Project -> Issue -> Thread`。本设计在其下增加第四层：

```text
Project
  -> Issue
      -> Run           一次完整的交付尝试
          -> Phase     plan / code
              -> Cycle 一次 [产出 -> 评审 -> 回应] 交换
```

分工不变：Thread 记录**对话过程**，Run 推进**交付状态**。Run 内部的每次 agent 执行仍然落到 Thread 和 Session，所以事后追溯细节的路径不变。

## 3. 人工介入点

只有三个，其余全部自动：

```text
【介入 1】需求澄清（交互式对话）
     人和一个 agent 聊清楚要做什么，产出 brief.md
     ↓ 以下自动
  planner 写 plan → reviewer 并行评审 → planner 回应并修订 → 循环直到停止
     ↓
【介入 2】审批 plan
     看未解决争议 → Approve / 打回并附意见
     ↓ 冻结 plan commit，创建 worktree
  implementer 实现 → 测试/lint/build → reviewer 并行评审 → 修复 → 循环直到停止
     ↓
【介入 3】审批代码
     看 diff + 测试报告 + 未解决争议 → Approve → 开 Draft PR
```

需求澄清**不应该**自动化。需求没谈清楚，后面所有轮次都是在精修一个错的东西——这是整条流水线里人的判断价值最高的一步。自动化从需求确定之后开始。

## 4. 文件系统是总线

不引入 artifact 数据库。agent 之间通过**磁盘上的文件**交换内容，编排器只传递路径和 prompt，不传递内容本身。

```text
<worktree>/.agentchat/runs/<run-id>/
  brief.md                    需求（人确认过）
  plan.md                     planner 产出，逐版覆盖
  reviews/r1/opus.json        每个 reviewer 写到指定路径
  reviews/r1/deepseek.json
  reviews/r2/...
  findings.jsonl              系统归并后的规范 finding
  dispositions.json           作者对每条 blocking 的回应
  followups.md                未采纳的建议，审批后转 Issue
  run.json                    状态机快照，用于恢复
```

这样做的理由：

- **review 质量**。reviewer 是在真实仓库里运行的 CLI agent，能 grep、能读相邻文件、能跑测试。把 plan 当成文本 blob 塞进 prompt，review 质量会显著下降。
- **版本免费**。每个阶段结束 `git commit` 一次，plan 冻结就是一个 commit sha，不需要自己实现版本和 hash。
- **可检查**。出问题时直接看目录，不需要查数据库。

**worktree 由人手动准备，不由 App 管理。** 你自己 `git worktree add` 建好隔离目录，然后在里面启动 daemon，App 只认它拿到的工作目录。这砍掉了一整个模块，也避免了 App 去猜你想怎么组织分支。

配套硬规则仍然成立：**同一时刻只有 implementer 对工作目录有写权限**。reviewer 只读，所以它们可以并行跑在同一个目录里，不需要各自的副本。

另一条：agent **不在流式输出里返回 JSON**，而是写文件到指定路径，由系统读文件做 schema 校验。Codex / opencode / Claude Code 的流式格式各不相同，走文件是唯一稳定的方式。

## 5. Finding：严重度由 schema 决定，不由模型自称

如果只在 prompt 里要求"标注 severity"，得到的结果是不可比的：不同模型对"high"的校准完全不同。校准不一致的标签等于没有标签，据此设计的门禁会失效。

因此严重度是**封闭类别集合 + 机械校验**的结果。

### 5.1 类别

阻塞类别（`BlockingCategory`）——只有这六种能阻塞：

| 类别 | 含义 |
| --- | --- |
| `contradicts_brief` | 与 brief 明确要求相悖 |
| `missing_requirement` | 遗漏 brief 中写明的需求 |
| `incorrect` | 会产生错误行为、崩溃或错误输出 |
| `breaks_existing` | 破坏既有测试或已记录的行为 |
| `security` | 安全问题 |
| `data_loss` | 数据丢失 |

非阻塞类别（`NonBlockingCategory`）：`style` / `naming` / `perf_hint` / `test_gap` / `refactor` / `readability` / `other`。

`refactor`、`style`、`test_gap` 这类建议在**结构上**进不了 blocking 数组，所以"不阻碍、不引入 bug 的建议可以不采纳"不需要 planner 做判断，系统已经替它拒绝了。

### 5.2 Reviewer 输出格式

```json
{
  "reviewer": "opus",
  "round": 1,
  "blocking": [
    {
      "category": "incorrect",
      "location": "daemon/core/src/run/budget.rs:88",
      "problem": "cycle 上限判断少算一轮",
      "evidence": "max_cycles=2 时 ledger 允许第三次修订进入 budget.rs",
      "recommendation": "把 > 改成 >="
    }
  ],
  "non_blocking": [
    { "category": "test_gap", "location": "...", "problem": "...", "evidence": "", "recommendation": "..." }
  ]
}
```

两个数组用同一种条目结构，reviewer 只需要学一种形状。

### 5.3 Validator 规则

对 `blocking` 数组逐条检查，违规一律**降级**到 `non_blocking` 的 `other`，并记录 `demoted_from`：

| 违规 | `demoted_from` |
| --- | --- |
| `category` 不在阻塞枚举内 | `unknown_category` |
| evidence 为空、少于 20 字符、与 problem 归一化后相同、或不含任何具体指代 | `weak_evidence` |
| 该报告已有 5 条 blocking 存活 | `over_blocking_limit` |

"具体指代"的判定是确定性的：evidence 中出现 `/`、`::`、反引号、下划线、任意数字，或一个长度大于 3 且含 `.` 的 token。

三点说明：

1. **降级而不是打回重跑。** 打回要重新调用 agent，花钱；降级是免费且确定的。激励方向也对：虚报严重度的 reviewer 只会失去影响力，不会让你多付一分钱，而且不必告诉它。
2. **5 条上限统计的是存活数，不是条目序号。** 前面几条因证据不足被降级，不应该占用后面条目的名额。
3. **上限本身是必需的。** 不设上限时模型会产出无限长的 nitpick 尾巴，而 nitpick 正是让循环停不下来的东西。

### 5.4 Finding 身份与分组

```text
finding_id = sha256(归一化文件路径 ␟ 类别 ␟ 归一化 problem) 取前 12 位十六进制
```

归一化：小写 + 折叠空白；文件路径额外剥离尾部的 `:88` 或 `:88-92` 行号。

`finding_id` 的用途**只有一个**：跨轮次身份稳定。系统会把上一轮的原文回灌给 reviewer，它复述时 id 相同，据此可以算出"本轮新增的 blocking"。

**跨 reviewer 不做文本合并。** 不同模型描述同一个问题用词不同，hash 必然不同；中文没有空格，基于 token 的相似度也不可靠。因此改为按 `(文件, 类别)` **分组**，组内保留每个 reviewer 的原文，`consensus` = 组内不同 reviewer 数。

这样拿到了最有用的排序信号——"有几个 reviewer 独立指向了同一处"——而不伪造语义精度。语义合并留到后续版本。

## 6. Disposition：作者必须逐条回应

```json
{
  "round": 1,
  "dispositions": [
    { "finding_id": "3f2a91c04d7b", "action": "accepted", "reason": "", "changed_files": ["daemon/core/src/run/budget.rs"] },
    { "finding_id": "8c1de55a0072", "action": "disputed", "reason": "并发失败路径在 v2 处理，brief 第 3 条已排除", "changed_files": [] }
  ]
}
```

门禁规则：

| 对象 | 要求 |
| --- | --- |
| blocking finding | 必须有 disposition，`accepted` 或 `disputed`；`disputed` 必须带非空 reason |
| blocking finding 标 `declined` | 拒绝——阻塞项只能接受或反驳 |
| non-blocking finding | **不要求** disposition。未提及的一律隐式不采纳，进 `followups.md` |
| 指向本轮不存在的 finding_id | 拒绝 |

非阻塞不设门禁，是因为要求作者逐条回应三十条 nitpick 纯粹浪费一个 turn，而非阻塞建议本来就不该给作者制造工作。

门禁不通过时，系统把缺失/无效的 finding_id 清单直接回灌给作者重写，**且这次不消耗 cycle 预算**（它没有推进任何事情）。

### 6.1 轮次 scope 递减

```text
round 1   全量评审        reviewer 之间互相看不到意见（保证视角多样性）
round 2   增量评审        只看本轮 diff + 上一轮被 disputed 的条目
                          能看到彼此上一轮的意见（避免重复提）
                          明确要求：不要重提已 accepted 的
round 3+  仲裁            单个 reviewer，只看 standoff
```

被 disputed 的条目只复核**一次**。reviewer 复核后仍坚持的，标记为 **standoff**，不再讨论，直接进审批包交给人。

## 7. Cycle 预算

### 7.1 计数单位

```text
cycle = [作者产出 vN] → [reviewer 扇出一轮] → [作者 disposition + 产出 vN+1]
```

初稿 v1 不计数。所以 `max_cycles = 2` 表示 plan 最多被改 2 次、最终交付 v3。这一个数字同时约束了"讨论次数"和"改 plan 次数"——它们是同一个循环的两个面。

**不用 token 或成本做门禁。** 各家 CLI 是否上报用量不可靠，ACP 0.10 也不保证。要控制的本来就是迭代次数，直接数迭代次数既准确又可解释。

### 7.2 什么不消耗预算

如果重试也算 cycle，一个偶发抽风的 agent 就能吃掉整个额度，结果是 plan 只被真正评审了一次就交上来。因此区分"推进"和"没推进"：

| 情况 | 消耗 cycle | 免费重试次数 | 用尽后 |
| --- | --- | --- | --- |
| reviewer 输出 schema 非法 | 否 | 1 | 把该 reviewer 踢出本轮，用其余的继续 |
| agent 崩溃 / 超时 / 触发看门狗 | 否 | 2 | 同上 |
| 作者未通过 disposition 门禁 | 否 | 1 | 判定 `stuck`，进审批 |
| 作者产出新版本且至少一个 reviewer 完成评审 | **是** | — | — |

一句话：只有真正发生了"意见 → 修改"的信息交换才扣。

### 7.3 停止条件

四条路都通向同一个审批点，区别只是审批包里"未解决争议"一节的内容：

```text
consecutive_revisions_without_review > 1   → stuck      作者自循环，评审没进来过
new_blocking(n) == 0                       → converged  收敛，争议区为空
n >= 2 && new_blocking(n) >= new_blocking(n-1) → churn  提前止损
cycles_used >= max_cycles                  → cycle_cap  预算用尽，列出所有争议
```

判定顺序如上，因此收敛优先于预算用尽：最后一个 cycle 同时触顶且清零时判 `converged`。

**churn 检测是省钱的主力。** 第 2 轮开始 scope 已经收窄到 diff + disputed，本该显著变少；如果反而持平或更多，说明要么 plan 方向就错了，要么 reviewer 在无限展开细节——这两种情况继续跑都是纯烧钱。这条规则会在第 2 轮就杀掉一个注定失败的 run，而不是烧到第 4 轮。

单纯的"最多 N 轮"不是收敛机制，只是截断：N 轮之后问题还在，人还是得介入。递减 scope + 明确的退出理由才让每条路径都有确定的终点。

### 7.4 人工打回

人在审批点选择"打回并附意见"时：`cycles_used` 归零、免费重试额度重置、`human_iterations += 1`。

理由：模型之间反复拉扯是空转，人注入新信息不是空转，值得给一轮完整的新预算。`human_iterations` 只记录、不设上限——人自己会烦，不需要系统替他限制。

### 7.5 看门狗（不是预算）

`stage_timeout_secs` 和 `stage_max_tool_calls` 用于防止单个 agent 跑飞（陷入循环无限读文件、等一个永不返回的命令），不用于控制成本。触发后杀掉该 stage 进程，按 `agent_failure` 走免费重试。

采集点是 `ResponseEvent::ThreadAgentToolUpdate`（`daemon/protocol/src/lib.rs`），已在协议中，不需要改 backend。

## 8. 审批包

审批页的信息架构按"渐进披露"组织，把人的注意力放在唯一需要判断的地方：

```text
├─ 决策区（必读，通常 < 10 行）
│    未解决争议 standoff
│    计划变更请求（如有）
│
├─ 本轮讨论摘要（一眼扫过）
│    blocking     12 提出 → 9 采纳 / 2 争议已解决 / 1 standoff
│    non-blocking 31 提出 → 4 转 followup / 27 未采纳
│    退出原因：churn
│
├─ plan.md 全文 / diff（折叠）
│
└─ 讨论存档（折叠，按 consensus 降序）
     ▸ non-blocking · ≥2 人提及   ← 默认展开
     ▸ non-blocking · 单人提及
     ▸ 已采纳的 blocking
```

三条约束：

1. **汇总由系统机械拼装，不经过 planner。** planner 决定了哪些不采纳，让它复述这些建议存在利益冲突；而且这纯粹是数据渲染，多花一个 agent turn 还会引入转述失真。planner 只提供它本来就要写的 reason，其余逐字取自 `findings.jsonl`。
2. **每条建议旁边要有动作**：`转 followup Issue` / `强制采纳并打回` / `忽略`。没有动作它就只是"读"，浪费时间；有动作，30 秒浏览直接转化为 backlog。
3. **planner 的 declined reason 与建议并排显示。** 理由写得糊弄或明显没理解建议，说明这个 planner 在本任务上不可靠——这个信号别处拿不到。

### 8.1 讨论存档的跨 run 价值

单次 run 里它是讨论历史；攒几次之后它是调参数据：

- 某个 reviewer 的 non-blocking 从未被采取任何动作 → 它的 prompt 有问题，或它不该出现在这个阶段
- 同一 category 每个 run 都出现 → 这是**模板**的系统性缺陷，改一次模板比每次 run 里处理一遍划算
- standoff 集中在某两个模型之间 → 风格冲突，换组合

因此存档路径要稳定、可跨 run 检索。当前目录结构已满足，用脚本汇总即可，不需要数据库。

## 9. Reviewer 配置

数量按阶段缩放，因为成本随输入规模走：

| 阶段 | reviewer 数 | 理由 |
| --- | --- | --- |
| plan 评审 | 3 | plan 很短，视角多样性在这一步收益最高 |
| 代码评审 | 2 | 要读仓库和 diff，最贵；仅当两者在同一位置结论相反时拉入第 3 个 |
| 仲裁 | 1 | 只看 standoff |

无差别地每个阶段上四个模型，是人肉调度成本高时的习惯。自动化之后应该反过来：**默认少，按需加。**

另外，模型不写死角色。配置的是能力角色（planner / reviewer / implementer / fixer / adjudicator），具体用哪个模型由配置决定。

### 9.1 Prompt 纪律

同一个模型，prompt 写成"找出所有可以改进的地方"和写成"只报会导致功能不正确或不满足需求的问题，最多 5 条，每条必须给出具体文件和可复现的证据"，收敛行为差一个数量级。

**Reviewer prompt 的纪律性比模型选择更决定系统是否收敛。** 因此 prompt 模板要做成文件、可版本管理、可 A/B 对比——沿用 `clients/apple/AgentChatPrototype/Config/distillation_prompt_template*.json` 的做法。

## 10. 分歧交给证据，不交给第四个模型

reviewer 之间结论相反时，不要引入更强的模型仲裁，而是尽量交给编译器和测试。

声称"这样会 break X"的 blocking finding，最好附带一个能复现的失败测试。有证据的 finding 权重高，纯断言的降级为 non-blocking。这比"升级到更强模型"可靠得多，也便宜得多。

## 11. 实现状态

已完成：

| 位置 | 内容 |
| --- | --- |
| `daemon/protocol/src/run.rs` | 类别枚举、`RawReviewReport` / `Finding` / `FindingGroup`、`Disposition`、`CycleBudgetConfig`、`ExitReason`、`PhaseKind` / `StageKind` / `RunStatus`、`ApprovalPacket` 及其视图类型、`finding_id()` |
| `daemon/core/src/run/findings.rs` | validator（降级规则）、归一化、分组与 consensus、`new_blocking_since()` |
| `daemon/core/src/run/disposition.rs` | 门禁、拒绝明细与回灌文本、disputed 与 followup 提取 |
| `daemon/core/src/run/budget.rs` | `CycleLedger`：cycle 计数、免费重试、停止条件、人工打回重置 |
| `daemon/core/src/run/state.rs` | `PhaseState` 三阶段状态机、`RunState` 与审批点转换 |
| `daemon/core/src/run/store.rs` | `run.json` 读写、重启后扫描与恢复、待审批筛选 |
| `daemon/core/src/run/approval.rs` | 审批包机械拼装：争议区、统计、followups、按 consensus 分层的讨论存档 |
| `daemon/core/src/run/layout.rs` | run 目录路径；reviewer 名做 slug 化，防止越出 reviews 目录 |
| `daemon/core/src/run/prompts.rs` | 六个阶段的内置 prompt 模板，可用 `.agentchat/prompts/<phase>_<stage>.md` 覆盖 |
| `daemon/core/src/run/executor.rs` | 阶段执行器：渲染 prompt、驱动 agent、读回文件、校验、免费重试、reviewer 并行与降级 |
| `daemon/core/src/run/gate.rs` | 审批门 trait + 文件版实现（写 markdown、轮询决策文件）、人工意见回写 brief |
| `daemon/core/src/run/orchestrator.rs` | 串起整个 run：阶段循环、每阶段落盘、两个审批门、断点恢复、`--plan-only` |
| `daemon/core/src/run/progress.rs` | 进度事件与终端渲染；agent 的工具调用实时可见 |
| `daemon/bin/src/main.rs` | `agentchat-daemon run` 子命令：角色解析、prompt 覆盖加载、驱动 |

`PhaseState` / `RunState` 放在 core 而不是 protocol，因为它们携带转换规则；两者都可直接序列化，`ws.rs` 需要下发时直接用即可，不必再造一份镜像类型。

### 11.1 现在怎么用

```bash
git worktree add ../feature-a -b feature-a
cd ../feature-a
agentchat-daemon run --brief ./requirement.md --plan-only   # 第一次建议加 --plan-only
```

`--plan-only` 在 plan 审批通过后就停下，**不会写你的工作树**——plan 阶段只读仓库、只写一个 markdown。验证过 agent 的行为之后，用同一个 `--run-id` 不带这个开关重跑即可续到 code 阶段。

角色默认：第一个 agent 写，其余 agent 审。想指定就用 `--planner` / `--plan-reviewers` / `--implementer` / `--code-reviewers`。

运行过程中终端会实时打印阶段、各 agent 的工具调用、每轮的新增 blocking 数、reviewer 被降级的原因。

跑到审批点会停下并打印路径，读 `.agentchat/runs/<id>/approval-plan.md`，然后：

```bash
echo '{"decision":"approve"}' > .agentchat/runs/<id>/decision-plan.json
echo '{"decision":"request_changes","comments":"..."}' > .agentchat/runs/<id>/decision-plan.json
```

中断后用同一个 `--run-id` 重跑即可从记录的 stage 续上。

尚未实现：

- WebSocket 协议扩展与图形化审批（现在是 markdown + 决策文件）
- 危险命令白名单——**这是无人值守运行 implementer 的前置条件**，不是后续功能。`daemon/core/src/capabilities.rs` 目前无条件 auto-approve 全部权限请求。

## 12. 明确的非目标

第一版不做，因为在还不知道流水线实际形状时固化抽象只会固化错误：

- **worktree / 分支管理**——人手动建好 worktree，在里面启动 daemon
- 任意 DAG 工作流编辑器
- 用户自定义 workflow 模板
- 基于成本或风险的模型路由策略
- 无约束的 agent 群聊
- 精确的 token / 美元计费

## 13. 一句话总结

```text
要做的不是让更多 agent 一起聊天，
而是让 agent 成为可编排、可审计、可暂停的流水线，
人只保留方案批准和代码批准两个决策点。
```
