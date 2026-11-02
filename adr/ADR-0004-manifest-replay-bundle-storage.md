# ADR-0004：Manifest、Replay Bundle 与本地存储

状态：已接受（M0，2026-11-03）

## 决策

### 1. Manifest ≠ 重放的全部输入
manifest 是**执行记录**；重放需要 **Replay Bundle**：

```
规范化请求
＋计算清单（manifest）
＋原始数据，或可持续定位的内容寻址引用
＋Provider/引擎配置
＋必要的环境与版本信息
```

M0 的请求自包含（参数内联），bundle 即清单文件本身；M1 引入 `load_market_dataset` 后数据以内容寻址引用进入 bundle。

### 2. 本地存储（M0 冻结）
- **一清单一文件 + 原子写入**（先写 `.tmp` 再 rename），不采用 JSONL（避免查找、并发写、半截记录复杂度）；
- 默认目录：`$FINSTRUMENT_DATA_DIR` → `$XDG_DATA_HOME/finstrument` → `~/.local/share/finstrument`；清单存于 `calculations/<calculation_id>.json`；
- 文件名即 `calculation_id`（查找 O(1)，无索引）；
- 保留策略：M0 不做自动清理（显式策略待真实需求）；
- 敏感数据不默认复制进 bundle；引用优先于内联。

### 3. 不可重放状态必须显式
- 找不到清单 → `replay_unavailable`（错误信封，correctable=false）；
- 清单在、引用的输入缺失 → `artifact_unavailable`；
- Provider 环境缺失 → `provider_unavailable`；
- **不得把"找到 manifest"表述为"可以重放"**。

### 4. 重放语义（同版本同环境）
重放 = 重新执行 bundle 内请求 → 核对 `result_digest` 一致 + 校验 `manifest_hash` 可重建 → 结构化差异报告（哪个指标、差多少、容差多少）。首版承诺仅限同版本同环境；跨版本只做清单解析与差异说明。

### 5. 标识与摘要分层（继承 ADR-0001 §3）
manifest 新增字段：`runtime_version`、`provider_id`、`provider_version`；`environment` 块仍排除在哈希域外。
