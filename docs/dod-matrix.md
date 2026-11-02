# 工具 × DoD 矩阵（公开验证矩阵，ADR-0003 §2 / ADR-0006 门禁 9）

> 状态标记：✅ 已验证｜🟡 部分满足｜⬜ 未开始（M1+）。数值工具与非数值工具的 DoD 按类型适用，不机械套用。

| 工具 | Oracle 证据 | 金融不变量 | 单位语义 | 错误路径 | Agent Eval | 清单/重放 | 文档 | 状态 |
|---|---|---|---|---|---|---|---|---|
| `price_european_option` | ✅ 18 组 QuantLib fixtures（含边界/近零/ATM） | ✅ put-call parity 网格 90 点、上下界、单调性 | ✅ 逐指标 units（Vega 每 1.00 vol、Theta 每年） | ✅ invalid_input / unsupported_feature / numerical_failure 定义 | ✅ 6 类用例 + parity 工作流 | ✅ manifest + 同环境重放 | ✅ 本文件 + Skill | **M0 交付** |
| `get_calculation_manifest` | —（非数值） | — | — | ✅ replay_unavailable（缺记录） | ✅ skill trace 覆盖 | ✅ 哈希校验后返回 | ✅ | **M0 交付** |
| `replay_calculation` | —（非数值） | — | — | ✅ replay_unavailable / artifact_unavailable | ✅ skill trace 覆盖 | ✅ 摘要重算 + 差异报告 | ✅ | **M0 交付** |
| 外部 Python Provider 通道 | —（工程原型） | — | — | ✅ provider_unavailable / resource_limit(超时) / internal_error(崩溃) | — | ✅ 版本上报入 manifest 通道就绪 | ✅ ADR-0002 | **M0 原型** |
| `load_market_dataset` | ⬜ schema/类型/日期序/重复/缺失/恶意 CSV/资源限制 | — | ⬜ 收益口径字段 | 🟡 规则已冻结（ADR-0005），实现 M1 | ⬜ 注入用例 M1 | ⬜ 数据以内容寻址引用入 bundle | 🟡 ADR-0005 | M1 |
| `optimize_portfolio` | ⬜ 解析 QP + KKT + 约束满足 + 目标值 | ⬜ 可行性/对偶间隙 | ⬜ weight/position 口径 | ⬜ | ⬜ | ⬜（Python Provider 经边车） | ⬜ | M1 |
| `analyze_portfolio_risk` | ⬜ 手算小样本 fixtures | ⬜ 年化一致性 | ⬜ simple/log、频率显式 | ⬜ | ⬜ | ⬜ | ⬜ | M1 |
| `stress_test_portfolio` | ⬜ 可手算持仓冲击 fixtures | ⬜ 情景基准一致 | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | M1 |

## M0 证据索引（对应 ADR-0006 十条门禁）

| 门禁 | 证据 |
|---|---|
| 1 三入口等价 | `crates/florin-cli/tests/conformance.rs`（lib/MCP/CLI 同请求同 result digest；同坏请求同错误信封） |
| 2 口径显式 | 结果 `conventions` + 逐指标 `units`；默认 Actual365Fixed/连续复利可显式覆盖 |
| 3 fixtures+不变量进 CI | `core/tests/black_scholes_fixtures.rs`（18 组）+ `bs.rs` 内 parity 网格/边界/零波动率极限 |
| 4 清单持久化 | `runtime/src/store.rs`（原子写：tmp+rename+fsync；路径遍历防护） |
| 5 重放与缺失态 | conformance 的 replay 往返 + `replay_unavailable` 路径测试 |
| 6 Skill trace | `skills/option-pricing-risk.md` + `evals/cases/skill_option_pricing_trace.json`（定价→清单→重放三步，$calculation_id 动态绑定） |
| 7 Python 边车原型 | `florin-sidecar`（冷启动握手/版本/echo_add 往返/超时→resource_limit/崩溃→internal_error） |
| 8 无隐式联网/无静默回退 | 代码审查（零首跑联网逻辑）+ 缺解释器→provider_unavailable 测试 |
| 9 本矩阵 | 本文件 |
| 10 合成数据 | `data/gen_synthetic_prices.py`（seed 20260101）+ `demo_prices.csv` + SHA256（README 记录） |

## 诚实声明

- `invalid_market_data` / `no_convergence` 已定义、M0 无触发场景（M1 数据加载启用）；
- 重放承诺 = 同版本同环境；跨版本仅解析与差异说明；
- Python Provider M0 是工程通道原型（echo_add），不含组合优化；
- WASM 仅为验证通道目标（M0–M2），未接入本仓库 CI（v1 已验证过该模式，M2 差分接入）。
