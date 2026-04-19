IL_INPUT  ?= samples/il_hedge_request.json
IL_OUTPUT ?= /tmp/il_hedge_output.json
SOL_INPUT  ?= samples/sol_autocall_request.json
SOL_OUTPUT ?= /tmp/sol_autocall_output.json

APP_PORT ?= 8787
BASELINE_DATE ?= $(shell date +%F)
BASELINE_OUT ?= research/precision_baseline_$(BASELINE_DATE).json
PRECISION_BASELINE ?= research/precision_baseline_2026-04-19.json
L2_CARGO_EXCLUDES = --exclude halcyon-wasm --exclude halcyon_flagship_quote --exclude halcyon_flagship_autocall --exclude halcyon_il_quote --exclude halcyon_il_protection
L2_ANCHOR_PROGRAMS = halcyon_kernel halcyon_stub_product halcyon_sol_autocall
.PHONY: bootstrap test check l2-cargo-check l2-cargo-test audit-check l2-gate il-hedge sol-autocall fmt-check app app-wasm anchor-build anchor-build-l2 localnet clean layouts-check anchor-test anchor-test-l2 precision-baseline precision-baseline-check

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

l2-cargo-check:
	cargo check --workspace $(L2_CARGO_EXCLUDES)

l2-cargo-test:
	cargo test --workspace $(L2_CARGO_EXCLUDES)

audit-check:
	test -f security/cargo_audit_waivers.md
	@echo "cargo audit waivers are documented in security/cargo_audit_waivers.md"
	cargo audit

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

anchor-build-l2:
	scripts/anchor_build_checked.sh $(L2_ANCHOR_PROGRAMS)

# L0: launches solana-test-validator with the Halcyon program IDs reserved.
localnet:
	./scripts/localnet.sh

clean:
	cargo clean

# L1 layer-boundary check: every IDL-exposed kernel account matches LAYOUTS.md.
layouts-check:
	@scripts/anchor_build_checked.sh halcyon_kernel
	@scripts/check_layouts.sh

# K11 — static regression guard: forbids seeds+bump on kernel-owned Account<T>
# at the product->kernel CPI boundary. See LEARNED.md.
cpi-seeds-check:
	@scripts/check_cpi_seeds.sh

# L1 localnet integration tests (requires `anchor test`).
anchor-test:
	anchor test

anchor-test-l2:
	anchor test --skip-lint

# L1 gate: everything that must hold before L2 can start.
l1-gate: cpi-seeds-check layouts-check
	@echo "l1-gate: all structural checks green"

# L2 gate per build_order_part4_layer2_plan.md: preserve the L0 bootstrap,
# keep structural kernel guards green, and pass the localnet L0-L2 suite.
l2-gate: l2-cargo-check l2-cargo-test audit-check cpi-seeds-check anchor-build-l2 precision-baseline-check
	@scripts/check_layouts.sh
	anchor test --skip-lint

precision-baseline:
	cargo test -p solmath-core --features full i64_trig_ -- --nocapture
	cargo test -p solmath-core --features full norm_cdf_fast_ -- --nocapture
	cargo test -p solmath-core --features full implied_vol_vector_recovery -- --nocapture
	HALCYON_BASELINE_DATE=$(BASELINE_DATE) HALCYON_GIT_HEAD=$$(git rev-parse HEAD) cargo run -p solmath-core --features full --example precision_baseline -- $(BASELINE_OUT)

precision-baseline-check:
	@tmp=$$(mktemp /tmp/halcyon-precision-baseline.XXXXXX.json); \
	HALCYON_BASELINE_DATE=$(BASELINE_DATE) HALCYON_GIT_HEAD=$$(git rev-parse HEAD) cargo run -p solmath-core --features full --example precision_baseline -- $$tmp; \
	python3 scripts/check_precision_baseline.py $(PRECISION_BASELINE) $$tmp; \
	rm -f $$tmp
