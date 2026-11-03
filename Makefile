.PHONY: all test fmt clippy regen demo provider clean

all: test

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

wasm:
	cargo build -p trustquant-wasm --target wasm32-unknown-unknown --release

test: fmt clippy wasm
	cargo test --workspace

# Manual, dev-time only (GPL oracle in .venv-fp; see docs/financepy-crosscheck.json)
crosscheck:
	.venv-fp/bin/python fixture-generator/financepy_oracle.py

demo:
	cargo build -p trustquant-cli
	mkdir -p $(TRUSTQUANT_DATA_DIR)
	./target/debug/trustquant run examples/price_call.json
	CID=$$(ls $(TRUSTQUANT_DATA_DIR)/calculations | head -1 | sed 's/\.json//'); \
	./target/debug/trustquant replay $$CID

provider:
	cargo build -p trustquant-cli
	./target/debug/trustquant provider check

regen:
	$(MAKE) -C fixture-generator regen
	python3 fixture-generator/portfolio_oracle.py fixtures/
	python3 data/gen_synthetic_prices.py

clean:
	cargo clean
	$(MAKE) -C fixture-generator clean
