# 合成演示数据（ADR-0005 §3 / ADR-0006 门禁 10）

- `demo_prices.csv`：3 个合成标的 × 500 个工作日收盘价，几何布朗运动生成。
- 生成器：`gen_synthetic_prices.py`，固定种子 `SEED = 20260101`，无网络、无真实市场数据。
- 重新生成必须逐字节复现：

```bash
python3 data/gen_synthetic_prices.py
sha256sum data/demo_prices.csv   # 必须等于下行
```

- `demo_prices.csv` SHA256：`0d15d274fd57bfd793663daa381b2cccce6739e9286e3342aa26b203db01efef`

数据日期序列为简单工作日序列（周一至周五），**刻意不使用任何市场日历语义**；M1 的 `load_market_dataset` 会按 ADR-0005 校验其 schema（ISO 日期、升序、无重复、数值列）。
