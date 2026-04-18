IL_INPUT ?= samples/il_hedge_request.json
IL_OUTPUT ?= /tmp/il_hedge_output.json
SOL_INPUT ?= samples/sol_autocall_request.json
SOL_OUTPUT ?= /tmp/sol_autocall_output.json

APP_PORT ?= 8787

.PHONY: test il-hedge sol-autocall fmt-check app app-wasm

test:
	cargo test -p halcyon-quote --test products_smoke

il-hedge:
	cargo run -p halcyon-quote --bin il_hedge_product -- $(IL_INPUT) $(IL_OUTPUT)

sol-autocall:
	cargo run -p halcyon-quote --bin sol_autocall_product -- $(SOL_INPUT) $(SOL_OUTPUT)

fmt-check:
	cargo fmt --check

app-wasm:
	cargo build --release --target wasm32-unknown-unknown -p halcyon-wasm
	cp target/wasm32-unknown-unknown/release/halcyon_wasm.wasm app/halcyon_wasm.wasm
	@ls -la app/halcyon_wasm.wasm

app: app-wasm
	@echo "Halcyon app → http://localhost:$(APP_PORT)/"
	@cd app && python3 -m http.server $(APP_PORT)
