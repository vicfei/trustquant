# ADR-0002：外部 Provider 协议与 Python 发行方式

状态：已接受（M0，2026-11-03）

## 决策

### 1. 三层区分（不得混同）
1. **Rust 进程内 Provider trait**：内置计算核心实现，进程内调用，零协议开销；
2. **外部 Provider 协议**：跨进程边界（M1 默认形态 = JSON Lines over stdio 子进程边车，**是推荐默认而非唯一答案**；嵌入式解释器、远程 Provider 留待后续 ADR）；
3. **公共工具契约**：Agent 看到的请求/结果/错误 schema，与由谁执行无关。

### 2. 外部 Provider 协议（v1 版协议）必备要素
- 启动即发 hello：`{type, protocol, provider, version, capabilities[]}`；
- 请求/响应：`{type:"request", request_id, tool, params}` → `{type:"response", request_id, status, result|error}`；
- `request_id` 与 `calculation_id` 在 Runtime 侧关联记录；
- 生命周期语义：启动超时、正常退出、崩溃、请求超时、取消；
- stdout 仅协议消息，stderr 仅日志；
- Provider 须上报自身与关键依赖版本；
- 工作目录、环境变量、数据文件边界由 Runtime 显式授予；
- **不可用即显式 `provider_unavailable`，禁止静默回退**到任何其他实现。

错误映射：启动失败/进程消失 → `provider_unavailable`；请求超时 → `resource_limit`（retryable）；请求中崩溃 → `internal_error`（retryable）。

### 3. Python 发行方式（M0 冻结顺序）
1. 一站式包**预置锁定环境**（依赖版本与来源锁定，记录环境摘要）；
2. 或提供**显式安装命令**（如 `trustquant provider install`）；
3. **不做首次计算时的隐式联网安装**——理由：破坏无凭据快速开始、安装结果随时间/镜像漂移、重放环境与首装不一致、崩溃更难解释；
4. 未安装 → `provider_unavailable` + 安装指引；
5. `uv` 是实现候选，**不写入长期公共协议**。

### 4. M0 原型范围
Python 边车只需验证工程通道：冷启动握手、版本上报、一次确定性往返计算、请求超时、进程崩溃、`provider_unavailable`（解释器缺失）。不提前实现组合优化功能。Python 路径可由 `TRUSTQUANT_PROVIDER_PYTHON` 覆盖（测试与异构环境用）。

## 后果
- 轻量核心 profile 不引入任何 Python 依赖（边车是独立 crate 与进程）。
- 后续嵌入式/远程 Provider 均以新 ADR 评审，不与首版并行实现多条路径。
