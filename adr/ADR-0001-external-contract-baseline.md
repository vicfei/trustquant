# ADR-0001：外部契约基线（继承 v1 并重新验收）

状态：已接受（M0，2026-11-03）｜来源：v1 项目（vicfei/quantrust 存档）ADR-0002/0003 的决策，经 v2 评审逐条重新验收

## 决策

### 1. 日期与枚举
- 外部日期 ISO 8601 `YYYY-MM-DD`，无时区；年份范围 1901–2199；时间戳类字段另行定义。
- DayCounter / 期权类型 / 引擎等一律**闭合版本化枚举**；未知已知名 → `unsupported_feature`，未知字符串 → `invalid_input`。

### 2. 错误分类（10 类）
v1 七类全部保留；v2 新增三个运行时状态类，作为错误信封的枚举值（同 major 内的枚举扩展）：

| error_type | 语义 | correctable | retryable |
|---|---|---|---|
| invalid_input / unsupported_feature / invalid_market_data / no_convergence / numerical_failure / resource_limit / internal_error | 同 v1 语义 | — | — |
| `provider_unavailable` | 所需 Provider 未安装/不可启动 | true（可显式安装） | true |
| `artifact_unavailable` | 清单引用的输入数据缺失或不可定位 | false | false |
| `replay_unavailable` | 清单本身缺失，无法进入重放 | false | false |

禁止以 NaN/Inf 或空结果静默表达失败。

### 3. 规范序列化与摘要
- canonicalization **v1**：serde_json 紧凑输出、键字典序、浮点最短往返、拒绝非有限数、字符串原样字节。
- 摘要算法 **BLAKE3**；禁用 `DefaultHasher`。
- `calculation_id`＝执行标识（服务层铸造，UUIDv4），`manifest_hash`＝清单内容完整性摘要，`input_digest`/`result_digest`＝内容指纹。**三者职责不得混用**；`environment` 块排除在 `manifest_hash` 哈希域之外。

### 4. 容差与单位
- 浮点比较 `|a−e| ≤ atol + rtol·|e|`；容差锚粒度 = **指标 × 尺度类型**；近零期望由 `near_zero_policy` 接管。
- 离散结果（日期、Calendar、Schedule）精确相等。
- 每个数值结果逐指标携带机器可读单位语义 `{dimension, scale, convention?, reference_notional?}`；组合域扩展：simple/log return、weight/position、收益频率与年化方式必须显式。

### 5. 版本政策
- `schema_version` 从 **0.1** 起步（新契约基线，包含上述枚举扩展）；同 major 向后兼容；新增枚举值/union 分支 = minor；语义变更 = major。
- 复现级别是**工具能力声明与结果元数据**，不是请求参数。

## 重新验收记录
- canonicalization/BLAKE3/id-hash 分离：v1 已有实现与测试，随本仓库代码重新落地并跑通测试即视为验收。
- 本 ADR 与 v1 存档的差异：错误枚举 +3、单位语义扩展条款、schema 版本重定为 0.1。
