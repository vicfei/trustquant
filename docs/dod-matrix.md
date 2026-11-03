# 工具 × DoD 矩阵（公开验证矩阵，ADR-0003 §2 / ADR-0006 门禁 9）

> 状态标记：✅ 已验证｜🟡 部分满足｜⬜ 未开始（M1+）。数值工具与非数值工具的 DoD 按类型适用，不机械套用。

| 工具 | Oracle 证据 | 金融不变量 | 单位语义 | 错误路径 | Agent Eval | 清单/重放 | 文档 | 状态 |
|---|---|---|---|---|---|---|---|---|
| `price_european_option` | ✅ 18 组 QuantLib fixtures（含边界/近零/ATM） | ✅ put-call parity 网格 90 点、上下界、单调性 | ✅ 逐指标 units（Vega 每 1.00 vol、Theta 每年） | ✅ invalid_input / unsupported_feature / numerical_failure 定义 | ✅ 6 类用例 + parity 工作流 | ✅ manifest + 同环境重放 | ✅ 本文件 + Skill | **M0 交付** |
| `get_calculation_manifest` | —（非数值） | — | — | ✅ replay_unavailable（缺记录） | ✅ skill trace 覆盖 | ✅ 哈希校验后返回 | ✅ | **M0 交付** |
| `replay_calculation` | —（非数值） | — | — | ✅ replay_unavailable / artifact_unavailable | ✅ skill trace 覆盖 | ✅ 摘要重算 + 差异报告 | ✅ | **M0 交付** |
| 外部 Python Provider 通道 | —（工程原型） | — | — | ✅ provider_unavailable / resource_limit(超时) / internal_error(崩溃) | — | ✅ 版本上报入 manifest 通道就绪 | ✅ ADR-0002 | **M0 原型** |
| `load_market_dataset` | ✅ ADR-0005 全规则单测（结构/数据/资源三分类判例） | — | ✅ schema 显式 | ✅ invalid_input/invalid_market_data/resource_limit/artifact_unavailable | ✅ 注入即数据用例 | ✅ 内容寻址数据集入库（dataset_id） | ✅ 本文件 + ADR-0005 | **M1 交付** |
| `optimize_portfolio` | ✅ 独立纯 Python oracle：解析 min-var（3 资产 + 2 资产内点）与内点切点基准；边界用例仅不变量（诚实标注） | ✅ 和为 1、[0,max_w] 界、求解器容差 | ✅ 权重口径 + mu/cov 估计器显式（compounding=False 钉正） | ✅ provider_unavailable/invalid_input/invalid_market_data | ✅ 多步工作流 + skill trace | ✅ Provider 版本入 manifest；同环境重放 PASS | ✅ provider-status.md | **M1 交付（实验 Provider）** |
| `analyze_portfolio_risk` | ✅ 手算微样本 + 合成数据全量 oracle | ✅ 年化/复利口径显式 | ✅ simple 收益、252、ddof=1 逐指标 units | ✅ invalid_input/invalid_market_data | ✅ skill trace | ✅ manifest + 重放 | ✅ | **M1 交付** |
| `stress_test_portfolio` | ✅ 可手算持仓冲击 oracle（3 情景） | ✅ 贡献之和=组合收益（逐情景断言） | ✅ 单位名义基准口径 | ✅ invalid_input（非法冲击） | ✅ skill trace | ✅ manifest + 重放 | ✅ | **M1 交付** |

| `bootstrap_yield_curve` | ✅ QuantLib 80 日历 fixtures + 12 曲线/互换 fixtures（3 场景：平/向上/倒挂；DF、连续零息 Act/365F、helper 残差 ~1e-13） | ✅ 存款解析式核对 | ✅ 逐指标 units（DF/连续零息口径显式） | ✅ invalid_market_data（报价域）/no_convergence（非正 DF）/invalid_input | ✅ rates_workflow eval | ✅ manifest+同环境重放 | ✅ | **M2 交付** |
| `price_vanilla_swap` | ✅ 9 互换案例（payer/receiver×期限×off/ATM；NPV/公平/双腿/固定腿逐票精确日期与年化） | ✅ 望远镜浮腿=DF差；ATM NPV=0 eval 断言 | ✅ 单位名义 NPV + 年化 30E/360 公平利率口径 | ✅ unsupported_feature（>10y）/invalid_input | ✅ rates_workflow（含 30y 拒绝与坏报价） | ✅ | ✅ | **M2 交付** |
| 原生/WASM 差分 | ✅ 利率链路（曲线 DF、互换 NPV/公平/日程）+ 期权 + 错误信封双侧一致（浮点按容差，ADR-0003） | — | — | — | — | — | ✅ | **M2 交付** |
| Provider 一致性 | —（工程） | — | — | ✅ stub provider 走同一协议通道，结构化拒绝保留语义 | — | — | ✅ ADR-0002 | **M2 交付** |
| Schema 政策 | — | — | — | ✅ 同 major 任意 minor 接受 / 异 major 显式拒绝 / 未知枚举=协商不崩溃（测试固定） | — | — | ✅ schema 0.2 | **M2 交付** |
| 第二 oracle（利率域，自研） | ✅ rates_oracle（纯标准库）：二分法 bootstrap + 现金流加总定价，DF 1e-11、望远镜 gap ~1e-16，**CI 自动** | ✅ 方法独立性（数值解 vs 闭式、加总 vs 恒等式） | — | — | — | — | ✅ fixtures/rates_oracle/ + provider-status | **M2 交付（已替换 GPL FinancePy 手动对照）** |

## M0 证据索引（对应 ADR-0006 十条门禁）

| 门禁 | 证据 |
|---|---|
| 1 三入口等价 | `crates/trustquant-cli/tests/conformance.rs`（lib/MCP/CLI 同请求同 result digest；同坏请求同错误信封） |
| 2 口径显式 | 结果 `conventions` + 逐指标 `units`；默认 Actual365Fixed/连续复利可显式覆盖 |
| 3 fixtures+不变量进 CI | `core/tests/black_scholes_fixtures.rs`（18 组）+ `bs.rs` 内 parity 网格/边界/零波动率极限 |
| 4 清单持久化 | `runtime/src/store.rs`（原子写：tmp+rename+fsync；路径遍历防护） |
| 5 重放与缺失态 | conformance 的 replay 往返 + `replay_unavailable` 路径测试 |
| 6 Skill trace | `skills/option-pricing-risk.md` + `evals/cases/skill_option_pricing_trace.json`（定价→清单→重放三步，$calculation_id 动态绑定） |
| 7 Python 边车原型 | `trustquant-sidecar`（冷启动握手/版本/echo_add 往返/超时→resource_limit/崩溃→internal_error） |
| 8 无隐式联网/无静默回退 | 代码审查（零首跑联网逻辑）+ 缺解释器→provider_unavailable 测试 |
| 9 本矩阵 | 本文件 |
| 10 合成数据 | `data/gen_synthetic_prices.py`（seed 20260101）+ `demo_prices.csv` + SHA256（README 记录） |

## 诚实声明（M1 更新）

- `invalid_market_data` 已由数据加载启用判例；`no_convergence` 仍定义未触发；
- 有界优化解以不变量验证，不以解析权重冒充（fixtures/provenance 注明）；
- PyPortfolioOpt 1.6 的 compounding 默认翻转已被显式钉正并有测试守护（docs/provider-status.md）；
- 重放承诺 = 同版本同环境；跨版本仅解析与差异说明；
- Python Provider M0 是工程通道原型（echo_add），不含组合优化；
- WASM 仅为验证通道目标（M0–M2），未接入本仓库 CI（v1 已验证过该模式，M2 差分接入）。
