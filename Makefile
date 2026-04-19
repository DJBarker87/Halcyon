IL_INPUT  ?= samples/il_hedge_request.json
IL_OUTPUT ?= /tmp/il_hedge_output.json
SOL_INPUT  ?= samples/sol_autocall_request.json
SOL_OUTPUT ?= /tmp/sol_autocall_output.json

APP_PORT ?= 8787

.PHONY: bootstrap test check il-hedge sol-autocall fmt-check app app-wasm anchor-build localnet clean layouts-check anchor-test

# L0 entry point: fresh clone → `make bootstrap` should leave the repo at the
# L0 exit criterion (every crate compiles, backtest replay passes).
bootstrap:
	cargo fetch
	cargo check --workspace --exclude halcyon-wasm
	cargo test  -p halcyon_sol_autocall_quote --test smoke
	cargo test  -p halcyon_il_quote           --test smoke
	$(MAKE) sol-autocall

test:
	cargo test -p halcyon_sol_autocall_quote --test smoke
	cargo test -p halcyon_il_quote           --test smoke

check:
	cargo check --workspace --exclude halcyon-wasm

il-hedge:
	cargo run -p halcyon_il_quote --bin il_hedge_product -- $(IL_INPUT) $(IL_OUTPUT)

sol-autocall:
	cargo run -p halcyon_sol_autocall_quote --bin sol_autocall_product -- $(SOL_INPUT) $(SOL_OUTPUT)

fmt-check:
	cargo fmt --check

app-wasm:
	cargo build --release --target wasm32-unknown-unknown -p halcyon-wasm
	cp target/wasm32-unknown-unknown/release/halcyon_wasm.wasm app/halcyon_wasm.wasm
	@ls -la app/halcyon_wasm.wasm

app: app-wasm
	@echo "Halcyon app → http://localhost:$(APP_PORT)/"
	@cd app && python3 -m http.server $(APP_PORT)

# L0: compiles four empty #[program] scaffolds to BPF.
anchor-build:
	anchor build

# L0: launches solana-test-validator with the Halcyon program IDs reserved.
localnet:
	./scripts/localnet.sh

clean:
	cargo clean

# L1 layer-boundary check: every IDL-exposed kernel account matches LAYOUTS.md.
layouts-check:
	@anchor build
	@scripts/check_layouts.sh

# K11 — static regression guard: forbids seeds+bump on kernel-owned Account<T>
# at the product->kernel CPI boundary. See LEARNED.md.
cpi-seeds-check:
	@scripts/check_cpi_seeds.sh

# L1 localnet integration tests (requires `anchor test`).
anchor-test:
	anchor test

# L1 gate: everything that must hold before L2 can start.
l1-gate: cpi-seeds-check layouts-check
	@echo "l1-gate: all structural checks green"
