# Vibe-Trading Options Lab × TrustQuant 口径对照表

> 差分前必读。本表来自 2026-11-12 的活体对照（DSH headless 内 Agent 双工具实测
> + 人工 MCP 探针复核），先对口径再谈数值——任何公开差异结论以此表为前提。
> Vibe 版本：vibe-trading-ai 0.1.19（MIT）；TrustQuant 0.2.0（QuantLib 1.33 钉死）。

## 结论先行

同参数下两者 NPV/Delta/Gamma **逐位一致**（15 位有效数字）；"差异"全部来自
Greeks 的报告单位约定，换算后完全吻合。这不是巧合——两边都是 BSM 解析解，
TrustQuant 侧经 QuantLib fixtures 钉死，因此本对照等于**用 QuantLib 基准为
Vibe 的期权实现做了交叉背书**（反向亦成立）。

## 逐项口径

| 项 | Vibe `analyze_options` | TrustQuant `price_european_option` | 换算关系 |
|---|---|---|---|
| NPV | 每单位标的（无乘数） | `units_of_spot`（每单位 spot） | 直接相等 |
| Delta | 每 1.00 spot | `per_unit_spot` | 直接相等 |
| Gamma | 每 spot² | `per_unit_spot_squared` | 直接相等 |
| **Vega** | **每 1 个百分点波动率**（0.3967 @基准例） | **每 1.00 波动率**（39.67，结果 units 字段显式声明 `per_unit_vol`） | `vibe = trustquant / 100` |
| **Theta** | **每自然日**（−0.01720） | **每年**（−6.2771，`convention: per year`） | `vibe = trustquant / 365` |
| 期限 T | `expiry_days / 365`（日历日） | `Actual365Fixed(估值日, 到期日)` | **由调用方保证一致**：`expiry_days` 必须传实际日历日差 |
| 股息 | 无参数（隐含 q=0） | `dividend_yield` 显式（连续复利） | 对照时 TrustQuant 侧取 q=0 |
| 利率 | 连续复利 | 连续复利（`conventions.rates` 声明） | 直接相等 |

## 已知陷阱（非差异，是语义）

1. **闰年日差**：日期对跨 2/29 时 Act365F 给 T=366/365，而"顺手填 expiry_days=365"
   仍是 T=1.0——这是输入语义差异不是引擎差异。`run_diff.py` 一律从日期对算
   日历日差再传 `expiry_days`，杜绝该伪差异。
2. **Vibe 输出 6 位小数**：容差 atol=1e-5；TrustQuant 全精度。
3. **Vibe 无股息参数**：对带股息的合约做对照时须用 q=0，或只比 NPV 形状。

## 运行

```bash
pip install vibe-trading-ai==0.1.19   # 或用既有 venv
VIBE_MCP=/path/to/vibe-trading-mcp python3 crosscheck/vibe_options/run_diff.py
```

产出 `report-latest.json`（逐案例双引擎数值 + mismatch 列表 + verdict）。
