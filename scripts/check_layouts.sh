#!/usr/bin/env bash
#
# Parity check between LAYOUTS.md and the compiled IDL.
#
# Extracts the account name + total payload bytes from
# `target/idl/halcyon_kernel.json` and asserts every account listed in
# LAYOUTS.md matches its declared total. Run at layer boundary via
# `make layouts-check`.

set -euo pipefail

IDL=target/idl/halcyon_kernel.json
LAYOUTS=programs/halcyon_kernel/LAYOUTS.md

if [ ! -f "$IDL" ]; then
  echo "error: $IDL not found. Run \`anchor build\` first." >&2
  exit 1
fi

python3 - "$IDL" "$LAYOUTS" <<'PY'
import json
import re
import sys

idl_path, layouts_path = sys.argv[1], sys.argv[2]

PRIM_SIZES = {
    "u8": 1, "i8": 1,
    "u16": 2, "i16": 2,
    "u32": 4, "i32": 4,
    "u64": 8, "i64": 8,
    "u128": 16, "i128": 16,
    "bool": 1,
    "pubkey": 32, "publicKey": 32,
}

def type_size(ty, type_defs):
    if isinstance(ty, str):
        if ty in PRIM_SIZES:
            return PRIM_SIZES[ty]
        if ty in type_defs:
            return struct_size(type_defs[ty], type_defs)
        raise ValueError(f"unknown primitive: {ty}")
    if "array" in ty:
        inner, n = ty["array"]
        return type_size(inner, type_defs) * n
    if "defined" in ty:
        name = ty["defined"]
        if isinstance(name, dict):
            name = name.get("name")
        if name not in type_defs:
            raise ValueError(f"unknown defined type: {name}")
        return struct_size(type_defs[name], type_defs)
    if "option" in ty:
        return 1 + type_size(ty["option"], type_defs)
    raise ValueError(f"unsupported type: {ty}")

def struct_size(td, type_defs):
    kind = td["type"]["kind"]
    if kind == "struct":
        total = 0
        for field in td["type"]["fields"]:
            total += type_size(field["type"], type_defs)
        return total
    if kind == "enum":
        return 1  # simple-tag enums only
    raise ValueError(f"unsupported kind: {kind}")

with open(idl_path) as f:
    idl = json.load(f)

type_defs = {t["name"]: t for t in idl.get("types", [])}
idl_accounts = {}
for acc in idl.get("accounts", []):
    name = acc["name"]
    # In IDL-v1 accounts point at a type definition by name.
    if "type" in acc:
        td = acc
    else:
        td = type_defs[name]
    idl_accounts[name] = struct_size(td, type_defs)

doc_accounts = {}
current = None
with open(layouts_path) as f:
    for line in f:
        m = re.match(r"^##\s+(\w+)", line)
        if m:
            current = m.group(1)
        m2 = re.search(r"\*\*TOTAL\*\*\s*\|[^|]*\|\s*\*\*(\d+)\*\*", line)
        if m2 and current:
            doc_accounts[current] = int(m2.group(1))
            current = None

# Every account the IDL exposes must match LAYOUTS.md. LAYOUTS.md is allowed
# to list more (kernel declares types that L2+ instructions will surface).
missing_in_doc = set(idl_accounts) - set(doc_accounts)
if missing_in_doc:
    print(f"LAYOUTS.md missing entries for IDL accounts: {sorted(missing_in_doc)}", file=sys.stderr)

bad = []
for name, idl_size in idl_accounts.items():
    doc_size = doc_accounts.get(name)
    if doc_size is None:
        bad.append((name, "missing_in_doc", idl_size, None))
    elif doc_size != idl_size:
        bad.append((name, "size_mismatch", idl_size, doc_size))

if bad:
    for name, reason, idl_size, doc_size in bad:
        print(f"  {name}: {reason} (idl={idl_size}, doc={doc_size})", file=sys.stderr)
    sys.exit(1)

print(
    f"layouts-check: {len(idl_accounts)} IDL accounts match LAYOUTS.md "
    f"(LAYOUTS.md additionally documents {len(doc_accounts) - len(idl_accounts)} "
    f"kernel accounts not yet surfaced in an instruction)"
)
PY
