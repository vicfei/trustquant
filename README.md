# TrustQuant

> 开放、本地优先、面向 Agent 的金融工具层：**可信数值内核 → 统一工具契约 → Provider 路由 → 计算清单与重放**。
> 当前状态：**M0 Walking Skeleton**（库 + Runtime + CLI + MCP + Skill + 清单 + Eval 闭环）。
> 定名 **TrustQuant**（可信量化：数值证据、可审计、可重放）；许可证 **Apache-2.0**。

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
