# ADR-0005：CSV 数据质量规则与错误映射

状态：已接受（M0，2026-11-03）｜生效实现随 M1 `load_market_dataset` 交付；规则现在冻结以便 schema 先行

## 决策

### 1. 数据质量规则（首版清单）
1. 编码必须 UTF-8（BOM 容忍）；解析失败 → `invalid_input`；
2. 表头必填且与声明 schema 匹配（列名、顺序）；不匹配 → `invalid_input`（field_path 指向列）；
3. 类型强制：数值列不可解析 → `invalid_market_data`（带行号/列名）；
4. 日期列必须 ISO 8601 且**升序、无重复**；违反 → `invalid_market_data`；
5. 缺失值：允许显式空串/`nan`，规则按列声明（拒绝或容忍）；违反声明 → `invalid_market_data`；
6. 单元格内容**永远当数据**：包含指令样文本不执行、不改变行为（提示注入防护）；
7. 行数/文件大小/字段长度超限 → `resource_limit`（不是 invalid_input）；
8. 解析耗时超预算 → `resource_limit`（retryable）。

### 2. 判例映射表（invalid_input vs invalid_market_data）
- 结构性错误（编码、表头、schema 不符）＝请求构造问题 → `invalid_input`；
- 数据性错误（数值不可解析、日期乱序、缺失值违反列规则）＝数据不满足要求 → `invalid_market_data`；
- 资源性错误（大小、行数、耗时）→ `resource_limit`。

### 3. 演示数据纪律（M0 生效）
所有随包演示数据为**合成数据**：生成器 + 固定种子入库，产物 CSV + SHA256 摘要 + 来源说明（`data/README.md`）。不随包分发任何真实市场数据。
