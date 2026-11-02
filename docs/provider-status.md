# Provider 状态（公开、显式、不静默回退）

| Provider | 状态 | 服务工具 | 版本记录位置 | 不可用时 |
|---|---|---|---|---|
| `builtin-rust` | **稳定** | `price_european_option`、`analyze_portfolio_risk`、`stress_test_portfolio`、`load_market_dataset`（runtime）、manifest/replay | manifest `provider_version` = core 版本 | 不适用（进程内） |
| `python-sidecar` | **实验（M1）** | `optimize_portfolio` | manifest `provider_version` = `python-sidecar/<py> pypfopt=<v> cvxpy=<v>`（来自握手上报） | `provider_unavailable` + 安装指引（`trustquant provider install`），**不回退**到任何内置优化 |

## python-sidecar 锁定环境（ADR-0002）

PyPortfolioOpt==1.6.0、cvxpy==1.6.7、numpy==2.2.6、scipy==1.16.3、pandas==2.3.2（sidecar/requirements.txt，全 pin）。
显式安装：`trustquant provider install`（可设 `PIP_INDEX_URL` 走镜像）——这是唯一联网步骤；首次计算永不隐式安装。
通道语义：冷启动握手 → 请求/响应（`request_id` 关联）→ 超时→`resource_limit`（60s）→ 崩溃→`internal_error`（自动重启一次）→ 拒绝→保留 Provider 的 error_type/字段/安装提示。

## 语义钉正记录（重要）

**PyPortfolioOpt 1.6.0 将 `mean_historical_return` 默认改为 `compounding=True`（几何年化）**——与本项目声明的"算术平均日简单收益 ×252"口径不符。边车已显式 `compounding=False` 钉正（sidecar/trustquant_provider.py），由 optimize fixtures 的独立解析基准守护。升级 PyPortfolioOpt 前必须重跑该测试。
