# TrustQuant

> 开放、本地优先、面向 Agent 的金融工具层：**可信数值内核 → 统一工具契约 → Provider 路由 → 计算清单与重放**。
> 当前状态：**M1 公开预览候选**——三 Demo 全链路（期权定价 / 组合优化 / 风险压力与重放），每个工具经 oracle 验证，每次计算可审计可重放。英文版：README（主）。
> 定名 **TrustQuant**（可信量化：数值证据、可审计、可重放）；许可证 **Apache-2.0**。

[![CI](https://github.com/vicfei/trustquant/actions/workflows/ci.yml/badge.svg)](https://github.com/vicfei/trustquant/actions/workflows/ci.yml)
[![验证入口](https://img.shields.io/badge/verification-开放入口-2e7d32)](docs/verify.md)

## 一分钟验证闭环

```bash
make test                                   # fixtures + 不变量 + 三入口一致性 + 边车 + Eval
TRUSTQUANT_DATA_DIR=/tmp/fin make demo     # 定价 → 计算清单 → 重放 (REPLAY: PASS)
make provider                               # Python 边车通道（冷启动/版本/往返）
```

一次真实调用的样子（三个入口同一实现、同一结果）：

```bash
trustquant run examples/price_call.json    # 库/CLI/MCP 三入口一致性有契约测试
trustquant replay <calculation_id>         # 同版本同环境重放 + 摘要核对
trustquant mcp                             # stdio MCP 服务器（协议转换，无计算逻辑）
trustquant-eval run                        # 7 条确定性 Agent Eval（含 Skill trace）
```

结果信封携带：数值 + 逐指标单位语义（Vega=每 1.00 波动率、Theta=每年）、模型与口径（BSM、Act/365F、连续复利）、引擎/Provider/版本、`calculation_id` 与 `manifest_hash`。

## 为什么可信

- **Oracle**：18 组 QuantLib 1.33 golden fixtures（call/put × 价内外平价、1 天～10 年、波动率 1e-6～0.9、零利率、负利率）+ put-call parity 网格（90 点）+ 边界与零波动率极限测试；
- **分层信任**：核心类型与数值验证（fixtures/不变量）＋ Provider 协议（显式路由、无静默回退）＋ 运行时证据（manifest：谁算的、什么版本、什么输入）＋ 审计重放（digest 核对 + 差异报告）；
- **诚实状态**：美式期权 → `unsupported_feature`，不编造数值；Provider 缺失 → `provider_unavailable`，不静默回退；清单缺失 → `replay_unavailable`，不把"找到记录"说成"可以重放"。

## 生态位与近邻（2026-11-07）

"Agent 工作流之下的可信计算层"是真实但人烟稀少的生态位。最近的同行（尊重事实、各有取舍）：

| 项目 | ★ | 模式 | 对方的验证主张 | TrustQuant 的差异 |
|---|---:|---|---|---|
| [quantoracle](https://github.com/QuantOracledev/quantoracle) | 14 | 托管 API + MCP + CLI，免费额度后按调用付费（x402） | 63 个确定性计算器，对教科书数值"引用验证"，120 项基准 | **本地优先**（离线、无 key、无计量）；验证靠版本钉死的 **oracle 血统**（QuantLib 1.33 金样例 + CI 内独立第二 oracle），而非引用；逐次计算 manifest + 重放 |
| [tradingcalc-mcp](https://github.com/SKalinin909/tradingcalc-mcp) | 4 | 远程 streamable-HTTP MCP，免费 | 100 个工具；每条响应带 ECDSA 签名（可离线校验）+ 公开测试向量页 | 签名证明的是"该服务器说过 X"；**重放证明"任何人可重算出 X"**——信任锚在数学不在密钥；本地优先；深度钉死领域 + 单位语义 vs 广度 |
| [rateslib](https://github.com/attack68/rateslib) | 370 | Python 库（非 agent 向） | 固收数值的可信度标杆 | 我们的利率切片走同一条可信度优先路线（bootstrapping 对 QuantLib 1e-10）；rateslib 是可依赖的库，不是 agent 工具层 |

这一层之上是 agent 框架（TradingAgents ★121k、ai-hedge-fund ★64k、Vibe-Trading ★38k）——无一把数字钉在 oracle 上，这正是 TrustQuant 存在的原因（有实测非空谈：[裸 LLM vs oracle 实验](essays/llm-numbers-vs-oracle/README.md)）。数据类 MCP（akshare、yahoo-finance-mcp 等）是另一条赛道：供数据，不做可信计算。

本页每条主张都可查验——从[验证入口](docs/verify.md)开始。

公开验证矩阵见 [docs/dod-matrix.md](docs/dod-matrix.md)。

## 布局

| 路径 | 职责 |
|---|---|
| `crates/trustquant-core` | 数值核心：Date/DayCounter、BS 解析引擎、契约（10 类错误）、规范化序列化+BLAKE3、manifest 组装。无 I/O、无 async |
| `crates/trustquant-runtime` | 产品主体：工具注册/校验、manifest 存储（原子写）、重放、MCP 消息处理 |
| `crates/trustquant-sidecar` | 外部 Python Provider 边车客户端（JSON Lines over stdio，协议见 ADR-0002） |
| `crates/trustquant-cli` | 单一二进制 `trustquant`：run / replay / manifest / mcp / provider check |
| `crates/trustquant-eval` | 确定性 trace Agent Eval 运行器（解释评测为定期层入口，实验标记） |
| `fixture-generator/` | QuantLib C++ fixture 生成器（固定环境，字节级可重生成） |
| `skills/` + `evals/` | 官方 Skill #1（期权定价与风险解释）+ Eval 用例 |
| `data/` | 合成演示数据（固定种子，SHA256 入档） |
| `adr/` | ADR-0001..0006 |

## 边界（M0 明确不做）

组合优化功能（边车仅通道原型）、树/MC/第二引擎、日历体系、load_market_dataset（CSV 规则已冻结在 ADR-0005）、远程 HTTP serve、隐式联网安装、真实模型评测。重放承诺 = 同版本同环境。

## 血统

见 [RELEASE-LINEAGE.md](RELEASE-LINEAGE.md)：QuantLib 1.33（Ubuntu 24.04 apt 锁定）、rustc 1.99.1、python3.12；v1 存档（vicfei/quantrust@9d40ae9）的 ADR 与 fixture 生成器经重新验收后回收。

## 许可证

Apache-2.0（见 [LICENSE](LICENSE)）。代码复制进本仓库前必须审查来源许可证与 NOTICE 要求；GPL/AGPL 代码不得进入宽松许可核心。

## M1 更新（2026-11-03）

- 新增工具：`load_market_dataset`（ADR-0005 校验 + 内容寻址数据集）、`optimize_portfolio`（外部 Python Provider：PyPortfolioOpt 1.6.0 锁定环境，长仓 min_volatility/max_sharpe）、`analyze_portfolio_risk`、`stress_test_portfolio`（Rust 内置，纯 Python 独立 oracle 验证）。
- 三条官方 Skill（skills/）+ 11 条确定性 Agent Eval（含注入即数据、CSV 质量判例、多步工作流）。
- Web 本地展示（`trustquant serve`，仅 127.0.0.1）；`trustquant provider install` 显式安装（唯一联网步骤）。
- 已知限制与发现（含 PyPortfolioOpt 1.6 compounding 默认翻转的钉正、serde_json 1 ulp 解析特性）见 README(EN) 与 ADR-0001 §6。
