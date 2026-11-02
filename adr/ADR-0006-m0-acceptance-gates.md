# ADR-0006：M0 验收门禁（修订版）

状态：已接受（M0，2026-11-03）｜M0 = Walking Skeleton：库＋Runtime＋CLI＋MCP＋Skill＋清单＋Eval 闭环

## 十条门禁

| # | 门禁 | 证据形式 |
|---|---|---|
| 1 | 库、CLI、MCP 对同一请求返回等价结果和一致错误 | 三入口一致性测试（同一请求 → 同一 canonical result digest；同一坏请求 → 同一错误信封） |
| 2 | 期权日计数、利率口径及 Greeks 单位全部显式化 | 请求携带 day_counter/复利口径；结果逐指标带单位语义；文档写明 Vega=每 1.00 波动率、Theta=每年 |
| 3 | QuantLib fixtures、金融不变量和边界用例进入 CI | fixtures 消费测试 + put-call parity/边界性质测试 + 近零/边界 fixture 用例 |
| 4 | manifest、必要输入和配置能够持久化 | 原子写清单文件（含请求、配置、版本、Provider 信息） |
| 5 | 同版本同环境能够重放，缺少输入时返回明确状态 | replay 往返测试 + `replay_unavailable`/`artifact_unavailable` 路径测试 |
| 6 | 一条 Skill 通过确定性 trace Eval | skills/ + trace 文件在 CI 执行（脚本化调用方按 Skill 完成定价、解释要点检查、清单获取） |
| 7 | Python 边车完成冷启动、超时、崩溃和版本记录原型 | sidecar 集成测试（hello 握手/版本/往返/超时→resource_limit/崩溃→internal_error/缺解释器→provider_unavailable） |
| 8 | 不存在隐式联网安装和 Provider 静默回退 | 代码与配置审查（无任何首跑联网逻辑）+ 无回退测试 |
| 9 | 形成公开的工具×DoD 矩阵 | docs/dod-matrix.md 入库并随发布更新 |
| 10 | 演示数据采用可再分发的合成数据，生成器、固定种子、数据文件和摘要均可追踪 | data/ 生成器 + CSV + SHA256SUMS + README |

## 语义修正（相对初稿）
- **NullCalendar 不替代日计数惯例**：到期时间必须显式 DayCounter + 复利口径（M0 默认 Actual365Fixed + 连续复利，均可显式覆盖）；
- fixtures 数量按覆盖类别确定（call/put、价内/价外/平价、期限、波动率、近零与边界），不机械定值；
- "Agent 能按 Skill 完成任务" 在 M0 = **脚本化 Agent 调用方能按 Skill 的确定性 trace 完成定价、解释和清单获取**；真实模型解释能力属定期 Eval 层，不在此门禁内。

## M0 明确不做
树/MC/第二引擎、日历体系、组合优化功能、load_market_dataset 实现、远程 HTTP serve、隐式环境安装、真实模型评测。
