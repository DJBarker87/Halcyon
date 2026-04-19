#!/usr/bin/env python3

import json
import math
import sys


def load(path):
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def path_get(obj, path):
    cur = obj
    for part in path.split("."):
        cur = cur[part]
    return cur


def fail(message):
    print(f"precision-baseline-check: {message}", file=sys.stderr)
    sys.exit(1)


def require_true(candidate, path):
    if path_get(candidate, path) is not True:
        fail(f"`{path}` is not true")


def require_le(candidate, baseline, path, hard_limit, multiplier=1.10):
    got = float(path_get(candidate, path))
    base = float(path_get(baseline, path))
    allowed = max(hard_limit, base * multiplier)
    if math.isnan(got) or got > allowed:
        fail(f"`{path}` regressed: got={got} allowed={allowed} baseline={base}")


def require_ge(candidate, baseline, path, hard_floor, slack=0.0):
    got = float(path_get(candidate, path))
    base = float(path_get(baseline, path))
    allowed = min(hard_floor, base - slack)
    if math.isnan(got) or got < allowed:
        fail(f"`{path}` regressed: got={got} required>={allowed} baseline={base}")


def main():
    if len(sys.argv) != 3:
        print("usage: check_precision_baseline.py <baseline.json> <candidate.json>", file=sys.stderr)
        sys.exit(1)

    baseline = load(sys.argv[1])
    candidate = load(sys.argv[2])

    if candidate.get("status") != "pass":
        fail(f"candidate status is `{candidate.get('status')}`")

    for path in (
        "boundary.pass",
        "bs_full_hp.pass",
        "implied_vol.pass",
        "norm_fast.pass",
        "norm_poly.pass",
        "scalar_transcendentals.pass",
        "trig_public.pass",
    ):
        require_true(candidate, path)

    require_le(candidate, baseline, "scalar_transcendentals.exp_fixed_i.relative.max", 3.0e-8, 1.15)
    require_le(candidate, baseline, "norm_fast.overall_abs_error.max", 7.0e-5, 1.10)
    require_le(candidate, baseline, "norm_fast.interior_abs_error.max", 7.0e-5, 1.10)
    require_le(candidate, baseline, "norm_fast.mid_abs_error.max", 3.0e-6, 1.15)
    require_le(candidate, baseline, "norm_fast.tail_abs_error.max", 2.0e-6, 1.15)
    require_le(candidate, baseline, "norm_poly.cdf.max", 4.0, 1.0)
    require_le(candidate, baseline, "norm_poly.pdf.max", 2.0, 1.0)
    require_le(candidate, baseline, "trig_public.sin.max", 8.0, 1.0)
    require_le(candidate, baseline, "trig_public.cos.max", 8.0, 1.0)
    require_le(candidate, baseline, "trig_public.pythagorean_identity.max", 3.0, 1.0)
    require_le(candidate, baseline, "bs_full_hp.call.max", 1.0, 1.0)
    require_le(candidate, baseline, "bs_full_hp.put.max", 1.0, 1.0)
    require_le(candidate, baseline, "bs_full_hp.delta.max", 1.0, 1.0)
    require_le(candidate, baseline, "bs_full_hp.vega.max", 1.0, 1.0)
    require_le(candidate, baseline, "bs_full_hp.rho.max", 1.0, 1.0)
    require_le(candidate, baseline, "bs_full_hp.theta.max", 1.0, 1.0)
    require_le(candidate, baseline, "bs_full_hp.gamma.max", 0.0, 1.0)
    require_ge(candidate, baseline, "implied_vol.pass_rate", 0.91, 0.01)

    print("precision-baseline-check: candidate metrics within committed L2 envelope")


if __name__ == "__main__":
    main()
