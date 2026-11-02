.PHONY: all test fmt clippy regen demo provider clean

all: test

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test: fmt clippy
	cargo test --workspace

demo:
	cargo build -p florin-cli
	mkdir -p $(FLORIN_DATA_DIR)
	./target/debug/florin run examples/price_call.json
	CID=$$(ls $(FLORIN_DATA_DIR)/calculations | head -1 | sed 's/\.json//'); \
	./target/debug/florin replay $$CID

provider:
	cargo build -p florin-cli
	./target/debug/florin provider check

regen:
	$(MAKE) -C fixture-generator regen
	python3 data/gen_synthetic_prices.py

clean:
	cargo clean
	$(MAKE) -C fixture-generator clean
