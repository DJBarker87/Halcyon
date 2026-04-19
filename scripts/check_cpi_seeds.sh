#!/usr/bin/env bash
#
# K11 regression guard (LEARNED.md).
#
# Anchor 0.32.1 + SBF rustc 1.84 silently zeros subsequent fields on
# kernel-owned PDAs that carry `seeds = [...], bump` when the account is
# passed through a product->kernel CPI. The fix: drop `seeds + bump` on
# those specific `Account<T>` entries. This script greps the three CPI-
# boundary handlers and fails if a kernel-owned Account<T> is re-carrying
# a seeds constraint.
#
# Whitelist rules:
#   - `seeds + bump` on `init`/`init_if_needed` PDAs is fine (system_program
#     CPI runs first and the cached-T corruption hasn't manifested yet).
#   - `UncheckedAccount` with `seeds + bump` is fine — that's a signer-seeds
#     PDA, not a validated Account<T>.
#   - `TokenAccount`/`Mint` are SPL-owned, not kernel-owned. Seeds on them
#     derive the address but don't trigger the aliasing bug.
#
# Only `Account<'info, <KernelType>>` with `seeds + bump` and no `init` is
# rejected. `record_hedge_trade` is keeper-entry (not a CPI boundary) and
# is not checked by this script.

set -euo pipefail

LIFECYCLE=programs/halcyon_kernel/src/instructions/lifecycle

if [ ! -d "$LIFECYCLE" ]; then
  echo "error: $LIFECYCLE not found. Run from repo root." >&2
  exit 1
fi

python3 - "$LIFECYCLE" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
offenders = []

# Every `#[account]` struct declared in programs/halcyon_kernel/src/state/.
# These are the types the aliasing bug affects.
KERNEL_OWNED_TYPES = {
    "ProtocolConfig",
    "ProductRegistryEntry",
    "VaultState",
    "SeniorDeposit",
    "JuniorTranche",
    "PolicyHeader",
    "CouponVault",
    "HedgeSleeve",
    "HedgeBookState",
    "AggregateDelta",
    "Regression",
    "VaultSigma",
    "RegimeSignal",
    "FeeLedger",
    "KeeperRegistry",
    "LookupTableRegistry",
}

# CPI-boundary handlers only. record_hedge_trade is keeper-entry.
FORBIDDEN_FILES = [
    "reserve_and_issue.rs",
    "finalize_policy.rs",
    "apply_settlement.rs",
]

# Match each #[account(...)] attribute block and the following field.
# Group 1: attribute body. Group 2: field name. Group 3: full type.
attr_re = re.compile(
    r"#\[account\((?P<attrs>[^\]]*)\]\s*(?:pub\s+)?(?P<field>\w+)\s*:\s*(?P<ty>Account\s*<[^>]+>|UncheckedAccount\s*<[^>]+>)",
    re.DOTALL,
)

# Pull the inner type out of `Account<'info, <Type>>`.
inner_re = re.compile(r"Account\s*<\s*'[^,]+,\s*(\w+)")

for name in FORBIDDEN_FILES:
    path = root / name
    if not path.exists():
        continue
    text = path.read_text()
    for m in attr_re.finditer(text):
        attrs = m.group("attrs")
        field = m.group("field")
        ty = m.group("ty")
        if "seeds" not in attrs:
            continue
        if re.search(r"\binit\b|\binit_if_needed\b", attrs):
            continue
        if ty.startswith("UncheckedAccount"):
            continue
        m_inner = inner_re.search(ty)
        if not m_inner:
            continue
        inner = m_inner.group(1)
        if inner not in KERNEL_OWNED_TYPES:
            continue
        offenders.append((name, field, inner, attrs.strip()[:80]))

if offenders:
    print("K11 regression — kernel-owned Account<T> at CPI boundary with seeds+bump:", file=sys.stderr)
    for name, field, inner, attrs in offenders:
        print(f"  {name}::{field}  Account<'info, {inner}>  ({attrs}...)", file=sys.stderr)
    print(
        "\nSee LEARNED.md. Fall back to discriminator-based Account<T> validation.",
        file=sys.stderr,
    )
    sys.exit(1)

print("check_cpi_seeds: clean — no forbidden seeds+bump on CPI-boundary kernel-owned Account<T>")
PY
