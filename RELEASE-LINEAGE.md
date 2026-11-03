# RELEASE-LINEAGE

> 版本血统表。每次发版/基线变更必须更新并推送（铁律）。

| 里程碑 | tag | 日期 | 版本 | Rust 工具链 | QuantLib 基线 | Python | 底座 | 内容 |
|---|---|---|---|---|---|---|---|---|
| v1 M0（quantrust） | （未打 tag） | 2026-11-02 | 0.0.1 | rustc 1.98.1→1.99.1 | apt 1.33-1 | — | Ubuntu 24.04.4 / GH ubuntu-24.04 | 契约+fixtures+MCP/CLI+WASM 差分+Eval。**设计颠覆性改动后归档**：`vicfei/quantrust@9d40ae9`（私有） |
| v2 M0（trustquant，本项目） | （未打 tag） | 2026-11-03 | 0.0.1 | rustc 1.99.1 | apt 1.33-1（date_daycount fixtures 与 v1 字节一致重验收） | 3.12（边车通道原型） | Ubuntu 24.04.4 / GH ubuntu-24.04 | ADR-0001..0006；BS 解析引擎（18 组 fixtures + parity 网格）；Runtime（manifest 原子存储/重放/unavailable 三态）；Python 边车协议原型（冷启动/超时/崩溃/版本）；三入口一致性；Skill #1 + 7 条确定性 Eval；合成演示数据 |
| v2 M1 公开预览版（TrustQuant） | **v0.1.0** | 2026-11-03 | 0.1.0 | rustc 1.99.1 | apt 1.33-1（未变） | 3.12 + PyPortfolioOpt 1.6.0 / cvxpy 1.6.7 / numpy 2.2.6 / scipy 1.16.3 / pandas 2.3.2（全 pin） | Ubuntu 24.04.4 / GH ubuntu-24.04（CI 含 venv 与 quickstart --full） | 四新工具：load_market_dataset（ADR-0005 判例 + 内容寻址数据集）/ optimize_portfolio（边车，μ 口径钉正 compounding=False）/ analyze_portfolio_risk + stress_test_portfolio（Rust 内置，纯 Python oracle）；Skill #2/#3 + 11 条 Eval；Web 本地展示；provider install 显式安装；quickstart；**M1 发现并记录**：PyPortfolioOpt 1.6 compounding 默认翻转、serde_json 1 ulp 解析特性（ADR-0001 §6）、数据集并发写幂等修复 |

## 回收与重新验收记录

- `fixture-generator/`：来自 v1 存档，扩充 8 组 BS 类别用例（ATM/1 天/近零波动/深价内外/10 年/短高波动），重生成字节确定性已验证；
- `fixtures/date_daycount/`：与 v1 字节一致（重新验收 = 原样通过全部比对）；
- ADR-0001/0002/0003/0004/0005/0006：v1 ADR-0002/0003 的契约决策重新验收后收录于 ADR-0001，其余为 v2 新决策；
- `date.rs`/`daycount.rs`/`canonical.rs`/fixtures 比较器：v1 已验证实现回收，随新测试重跑通过即验收。

## 更名与许可证记录

- 2026-11-03：占位名 `finstrument` → 正式定名 **`trustquant`**（M1 公开前完成，GitHub 仓库同步更名，旧地址自动重定向）；crate/binary/环境变量（`TRUSTQUANT_DATA_DIR`、`TRUSTQUANT_PROVIDER_PYTHON`）同步更名。
- 2026-11-03：许可证确认为 **Apache-2.0**（`LICENSE` 全文入库）。

## 变更纪律

QuantLib 基线、工具链、Python 版本、底座镜像变更必须单独评审（ADR-0001/ADR-0003）并在此加行；fixtures 与合成数据重生成必须字节一致；tag 打在通过全部验收门禁的 commit 上。
