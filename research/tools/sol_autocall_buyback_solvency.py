#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import math
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from statistics import mean, median
from typing import Any


CURRENT_REPO_ROOT = Path("/Users/dominic/colosseumfinal")
LEGACY_RESEARCH_ROOT = Path("/Users/dominic/Colosseum")
DEFAULT_OUTPUT_DIR = CURRENT_REPO_ROOT / "research" / "sol_autocall_buyback_outputs"
DEFAULT_STEP_CSV = (
    LEGACY_RESEARCH_ROOT
    / "research"
    / "sol_autocall_hedged_sweep"
    / "outputs"
    / "parity_hedge_step_ledger.csv"
)
DEFAULT_ROW_ID = "CURRENT_V1_HEDGED_BALANCED"


FLOAT_FIELDS = {
    "close_price",
    "low_price",
    "close_ratio",
    "low_ratio",
    "target_hedge_ratio",
    "raw_delta",
    "clipped_delta",
    "policy_target_delta",
    "rebalance_action",
    "turnover",
    "fee_cost_usdc",
    "slippage_cost_usdc",
    "keeper_cost_usdc",
    "execution_cost_total_usdc",
    "trade_notional_usdc",
    "hedge_inventory_sol",
    "hedge_cash_usdc",
    "coupon_paid_usdc",
    "coupon_outflows_cumulative_usdc",
    "reserve_occupancy_usdc",
    "note_execution_cost_total_usdc",
    "note_net_vault_pnl_usdc",
    "note_commercial_edge_usdc",
    "note_knock_in_residual_usdc",
}

INT_FIELDS = {
    "entry_index",
    "entry_ts_ms",
    "exit_ts_ms",
    "step_index",
    "step_day",
    "step_ts_ms",
    "trade_count",
    "note_trade_count",
}

BOOL_FIELDS = {
    "observation_day",
    "autocalled",
    "missed_trade",
}


@dataclass
class ScenarioConfig:
    name: str
    notional: float = 1000.0
    knock_in_barrier: float = 0.70
    haircut_pct_of_notional: float = 0.10
    initial_ltv: float = 0.70
    liquidation_ltv: float = 0.85
    base_fee_bps: float = 10.0
    slippage_coeff: float = 25.0
    liquidity_proxy_usdc: float = 250_000.0
    keeper_bounty_usdc: float = 0.10
    stress_multiplier: float = 3.0
    stress_return_threshold: float = -0.05
    stress_vol_threshold: float = 1.00
    forced_fraction: float = 0.25

    @property
    def haircut_usdc(self) -> float:
        return self.notional * self.haircut_pct_of_notional

    @property
    def buyback_cap_usdc(self) -> float:
        return self.notional * self.knock_in_barrier - self.haircut_usdc


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a coupled book-level buyback solvency replay on the SOL production ledger export."
    )
    parser.add_argument("--step-csv", type=Path, default=DEFAULT_STEP_CSV)
    parser.add_argument("--row-id", default=DEFAULT_ROW_ID)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--notional", type=float, default=1000.0)
    parser.add_argument("--knock-in-barrier", type=float, default=0.70)
    parser.add_argument("--haircut-pct", type=float, default=0.10)
    parser.add_argument("--initial-ltv", type=float, default=0.70)
    parser.add_argument("--liquidation-ltv", type=float, default=0.85)
    parser.add_argument("--base-fee-bps", type=float, default=10.0)
    parser.add_argument("--slippage-coeff", type=float, default=25.0)
    parser.add_argument("--liquidity-proxy-usdc", type=float, default=250_000.0)
    parser.add_argument("--keeper-bounty-usdc", type=float, default=0.10)
    parser.add_argument("--stress-multiplier", type=float, default=3.0)
    parser.add_argument("--stress-return-threshold", type=float, default=-0.05)
    parser.add_argument("--stress-vol-threshold", type=float, default=1.0)
    parser.add_argument("--forced-fraction", type=float, default=0.25)
    return parser.parse_args()


def parse_row(raw: dict[str, str]) -> dict[str, Any]:
    parsed: dict[str, Any] = {}
    for key, value in raw.items():
        if key in FLOAT_FIELDS:
            parsed[key] = float(value)
        elif key in INT_FIELDS:
            parsed[key] = int(value)
        elif key in BOOL_FIELDS:
            parsed[key] = value == "true"
        else:
            parsed[key] = value
    return parsed


def iso_utc(ts_ms: int) -> str:
    return datetime.fromtimestamp(ts_ms / 1000, tz=timezone.utc).strftime("%Y-%m-%d")


def load_steps(
    step_csv: Path, row_id: str
) -> tuple[dict[str, list[dict[str, Any]]], dict[int, float]]:
    notes: dict[str, list[dict[str, Any]]] = defaultdict(list)
    price_by_ts: dict[int, float] = {}
    with step_csv.open(newline="") as f:
        reader = csv.DictReader(f)
        for raw in reader:
            if raw["row_id"] != row_id:
                continue
            row = parse_row(raw)
            notes[row["note_id"]].append(row)
            price_by_ts[row["step_ts_ms"]] = row["close_price"]
    for steps in notes.values():
        steps.sort(key=lambda row: row["step_index"])
    return notes, price_by_ts


def build_market_context(price_by_ts: dict[int, float]) -> tuple[dict[int, float], dict[int, float]]:
    ordered = sorted(price_by_ts.items())
    returns_1d: dict[int, float] = {}
    vol_5d_ann: dict[int, float] = {}
    for idx, (ts, px) in enumerate(ordered):
        if idx > 0:
            returns_1d[ts] = px / ordered[idx - 1][1] - 1.0
        if idx >= 5:
            rets = [
                math.log(ordered[j][1] / ordered[j - 1][1])
                for j in range(idx - 4, idx + 1)
            ]
            avg = sum(rets) / len(rets)
            var = sum((ret - avg) ** 2 for ret in rets) / len(rets)
            vol_5d_ann[ts] = math.sqrt(var) * math.sqrt(252.0)
    return returns_1d, vol_5d_ann


def compute_current_capital_mark(row: dict[str, Any], config: ScenarioConfig) -> float:
    return (
        config.notional
        + row["hedge_cash_usdc"]
        + row["hedge_inventory_sol"] * row["close_price"]
        - row["coupon_outflows_cumulative_usdc"]
    )


def compute_buyback_price(
    current_capital_mark: float, config: ScenarioConfig
) -> float:
    return max(
        0.0,
        min(config.buyback_cap_usdc, current_capital_mark - config.haircut_usdc),
    )


def estimate_unwind_cost(
    hedge_inventory_sol: float,
    close_price: float,
    multiplier: float,
    config: ScenarioConfig,
) -> float:
    trade_notional_abs = abs(hedge_inventory_sol * close_price)
    if trade_notional_abs <= 0.0:
        return 0.0
    slippage_bps = config.slippage_coeff * math.sqrt(
        trade_notional_abs / config.liquidity_proxy_usdc
    )
    total_cost_bps = (config.base_fee_bps + slippage_bps) * multiplier
    return (
        trade_notional_abs * total_cost_bps / 10_000.0
        + config.keeper_bounty_usdc * multiplier
    )


def build_book_view(
    notes: dict[str, list[dict[str, Any]]],
) -> tuple[dict[int, list[dict[str, Any]]], dict[str, int]]:
    by_ts: dict[int, list[dict[str, Any]]] = defaultdict(list)
    final_ts: dict[str, int] = {}
    for note_id, steps in notes.items():
        final_ts[note_id] = steps[-1]["step_ts_ms"]
        for row in steps[1:]:
            by_ts[row["step_ts_ms"]].append(row)
    return by_ts, final_ts


def prepare_active_states(
    rows: list[dict[str, Any]],
    ts: int,
    final_ts: dict[str, int],
    liquidated: set[str],
    borrow_amount: float,
    config: ScenarioConfig,
) -> list[dict[str, Any]]:
    active: list[dict[str, Any]] = []
    for row in rows:
        note_id = row["note_id"]
        if note_id in liquidated:
            continue
        if ts >= final_ts[note_id]:
            continue
        capital_mark = compute_current_capital_mark(row, config)
        buyback_price = compute_buyback_price(capital_mark, config)
        current_ltv = (
            borrow_amount / buyback_price if buyback_price > 0.0 else math.inf
        )
        active.append(
            {
                "row": row,
                "current_capital_mark_usdc": capital_mark,
                "buyback_price_usdc": buyback_price,
                "current_ltv": current_ltv,
            }
        )
    return active


def make_event(
    scenario: str,
    state: dict[str, Any],
    borrow_amount: float,
    live_notes_before: int,
    unwind_multiplier: float,
    returns_1d: dict[int, float],
    vol_5d_ann: dict[int, float],
    config: ScenarioConfig,
) -> dict[str, Any]:
    row = state["row"]
    ts = row["step_ts_ms"]
    unwind_cost = estimate_unwind_cost(
        row["hedge_inventory_sol"],
        row["close_price"],
        unwind_multiplier,
        config,
    )
    available_after_unwind = state["current_capital_mark_usdc"] - unwind_cost
    buffer = available_after_unwind - state["buyback_price_usdc"]
    coverage_ratio = (
        available_after_unwind / state["buyback_price_usdc"]
        if state["buyback_price_usdc"] > 0.0
        else None
    )
    return {
        "scenario": scenario,
        "note_id": row["note_id"],
        "entry_index": row["entry_index"],
        "event_ts_ms": ts,
        "event_date_utc": iso_utc(ts),
        "step_day": row["step_day"],
        "live_notes_before": live_notes_before,
        "close_ratio": row["close_ratio"],
        "low_ratio": row["low_ratio"],
        "close_price": row["close_price"],
        "hedge_inventory_sol": row["hedge_inventory_sol"],
        "hedge_inventory_mark_usdc": row["hedge_inventory_sol"] * row["close_price"],
        "hedge_cash_usdc": row["hedge_cash_usdc"],
        "coupon_outflows_cumulative_usdc": row["coupon_outflows_cumulative_usdc"],
        "current_capital_mark_usdc": state["current_capital_mark_usdc"],
        "buyback_price_usdc": state["buyback_price_usdc"],
        "borrow_amount_usdc": borrow_amount,
        "current_ltv": state["current_ltv"],
        "unwind_cost_usdc": unwind_cost,
        "available_after_unwind_usdc": available_after_unwind,
        "buffer_usdc": buffer,
        "coverage_ratio": coverage_ratio,
        "return_1d": returns_1d.get(ts),
        "vol_5d_ann": vol_5d_ann.get(ts),
        "stress_day": (
            returns_1d.get(ts, 0.0) <= config.stress_return_threshold
            or vol_5d_ann.get(ts, 0.0) >= config.stress_vol_threshold
        ),
        "autocalled_step": row["autocalled"],
        "trigger_reason": row["trigger_reason"],
        "unwind_multiplier": unwind_multiplier,
    }


def make_daily_row(
    scenario: str,
    ts: int,
    live_notes_before: int,
    todays_events: list[dict[str, Any]],
    returns_1d: dict[int, float],
    vol_5d_ann: dict[int, float],
    cumulative_liquidations: int,
    cumulative_buyback_paid_usdc: float,
    cumulative_unwind_cost_usdc: float,
    cumulative_buffer_usdc: float,
) -> dict[str, Any]:
    daily_buyback_paid = sum(event["buyback_price_usdc"] for event in todays_events)
    daily_unwind_cost = sum(event["unwind_cost_usdc"] for event in todays_events)
    daily_buffer = sum(event["buffer_usdc"] for event in todays_events)
    return {
        "scenario": scenario,
        "ts_ms": ts,
        "date_utc": iso_utc(ts),
        "live_notes_before": live_notes_before,
        "liquidated_today": len(todays_events),
        "live_notes_after": live_notes_before - len(todays_events),
        "stress_day": (
            returns_1d.get(ts, 0.0) <= -0.05 or vol_5d_ann.get(ts, 0.0) >= 1.0
        ),
        "return_1d": returns_1d.get(ts),
        "vol_5d_ann": vol_5d_ann.get(ts),
        "daily_buyback_paid_usdc": daily_buyback_paid,
        "daily_unwind_cost_usdc": daily_unwind_cost,
        "daily_buffer_usdc": daily_buffer,
        "min_event_buffer_usdc": min(
            (event["buffer_usdc"] for event in todays_events), default=None
        ),
        "max_event_ltv": max(
            (event["current_ltv"] for event in todays_events), default=None
        ),
        "cumulative_liquidations": cumulative_liquidations,
        "cumulative_buyback_paid_usdc": cumulative_buyback_paid_usdc,
        "cumulative_unwind_cost_usdc": cumulative_unwind_cost_usdc,
        "cumulative_buffer_usdc": cumulative_buffer_usdc,
    }


def rolling_five_day_window_metrics(daily: list[dict[str, Any]]) -> dict[str, Any]:
    if not daily:
        return {
            "worst_5d_liquidations": 0,
            "worst_5d_window_start_utc": None,
            "worst_5d_window_end_utc": None,
        }
    daily = sorted(daily, key=lambda row: row["ts_ms"])
    window_ms = 4 * 86_400_000
    best = {
        "worst_5d_liquidations": 0,
        "worst_5d_window_start_utc": None,
        "worst_5d_window_end_utc": None,
    }
    for start in daily:
        start_ts = start["ts_ms"]
        end_ts = start_ts + window_ms
        count = sum(
            row["liquidated_today"] for row in daily if start_ts <= row["ts_ms"] <= end_ts
        )
        if count > best["worst_5d_liquidations"]:
            best = {
                "worst_5d_liquidations": count,
                "worst_5d_window_start_utc": iso_utc(start_ts),
                "worst_5d_window_end_utc": iso_utc(end_ts),
            }
    return best


def summarize_replay(
    scenario: str,
    events: list[dict[str, Any]],
    daily: list[dict[str, Any]],
    total_notes: int,
) -> dict[str, Any]:
    if not events:
        return {
            "scenario": scenario,
            "coupled_book_replay": True,
            "total_notes": total_notes,
            "liquidated_notes": 0,
            "liquidated_fraction": 0.0,
            "buyback_possible_all": True,
            "failure_count": 0,
            "total_buyback_paid_usdc": 0.0,
            "total_unwind_cost_usdc": 0.0,
            "total_realized_buffer_usdc": 0.0,
            "min_buffer_usdc": None,
            "median_buffer_usdc": None,
            "mean_buffer_usdc": None,
            "min_coverage_ratio": None,
            "max_unwind_cost_usdc": None,
            "worst_single_day_buyback_count": 0,
            "worst_single_day_buffer_usdc": None,
            "worst_single_day_buffer_date_utc": None,
            **rolling_five_day_window_metrics(daily),
        }

    buffers = [event["buffer_usdc"] for event in events]
    failures = [event for event in events if event["buffer_usdc"] < 0.0]
    coverages = [
        event["coverage_ratio"]
        for event in events
        if event["coverage_ratio"] is not None
    ]
    worst_event = min(events, key=lambda event: event["buffer_usdc"])
    liquidation_days = [row for row in daily if row["liquidated_today"] > 0]
    worst_daily_buffer = min(
        liquidation_days, key=lambda row: row["daily_buffer_usdc"]
    )

    return {
        "scenario": scenario,
        "coupled_book_replay": True,
        "total_notes": total_notes,
        "liquidated_notes": len(events),
        "liquidated_fraction": len(events) / total_notes,
        "buyback_possible_all": len(failures) == 0,
        "failure_count": len(failures),
        "failure_note_ids": [event["note_id"] for event in failures],
        "total_buyback_paid_usdc": sum(event["buyback_price_usdc"] for event in events),
        "total_unwind_cost_usdc": sum(event["unwind_cost_usdc"] for event in events),
        "total_realized_buffer_usdc": sum(event["buffer_usdc"] for event in events),
        "min_buffer_usdc": worst_event["buffer_usdc"],
        "min_buffer_note_id": worst_event["note_id"],
        "min_buffer_date_utc": worst_event["event_date_utc"],
        "median_buffer_usdc": median(buffers),
        "mean_buffer_usdc": mean(buffers),
        "min_coverage_ratio": min(coverages) if coverages else None,
        "max_unwind_cost_usdc": max(event["unwind_cost_usdc"] for event in events),
        "worst_single_day_buyback_count": max(
            row["liquidated_today"] for row in liquidation_days
        ),
        "worst_single_day_buffer_usdc": worst_daily_buffer["daily_buffer_usdc"],
        "worst_single_day_buffer_date_utc": worst_daily_buffer["date_utc"],
        **rolling_five_day_window_metrics(daily),
    }


def run_primary_coupled_book_replay(
    notes: dict[str, list[dict[str, Any]]],
    returns_1d: dict[int, float],
    vol_5d_ann: dict[int, float],
    config: ScenarioConfig,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    by_ts, final_ts = build_book_view(notes)
    borrow_amount = config.initial_ltv * compute_buyback_price(config.notional, config)
    liquidated: set[str] = set()
    events: list[dict[str, Any]] = []
    daily_rows: list[dict[str, Any]] = []
    cumulative_buyback = 0.0
    cumulative_unwind = 0.0
    cumulative_buffer = 0.0

    for ts in sorted(by_ts):
        active = prepare_active_states(
            by_ts[ts], ts, final_ts, liquidated, borrow_amount, config
        )
        live_before = len(active)
        stress_day = (
            returns_1d.get(ts, 0.0) <= config.stress_return_threshold
            or vol_5d_ann.get(ts, 0.0) >= config.stress_vol_threshold
        )
        todays_events: list[dict[str, Any]] = []
        for state in active:
            if state["current_ltv"] < config.liquidation_ltv:
                continue
            todays_events.append(
                make_event(
                    scenario="primary_ltv_coupled_book",
                    state=state,
                    borrow_amount=borrow_amount,
                    live_notes_before=live_before,
                    unwind_multiplier=config.stress_multiplier if stress_day else 1.0,
                    returns_1d=returns_1d,
                    vol_5d_ann=vol_5d_ann,
                    config=config,
                )
            )

        for event in todays_events:
            liquidated.add(event["note_id"])
            cumulative_buyback += event["buyback_price_usdc"]
            cumulative_unwind += event["unwind_cost_usdc"]
            cumulative_buffer += event["buffer_usdc"]
        events.extend(todays_events)
        daily_rows.append(
            make_daily_row(
                scenario="primary_ltv_coupled_book",
                ts=ts,
                live_notes_before=live_before,
                todays_events=todays_events,
                returns_1d=returns_1d,
                vol_5d_ann=vol_5d_ann,
                cumulative_liquidations=len(events),
                cumulative_buyback_paid_usdc=cumulative_buyback,
                cumulative_unwind_cost_usdc=cumulative_unwind,
                cumulative_buffer_usdc=cumulative_buffer,
            )
        )

    return events, daily_rows


def run_stress_coupled_book_replay(
    notes: dict[str, list[dict[str, Any]]],
    returns_1d: dict[int, float],
    vol_5d_ann: dict[int, float],
    config: ScenarioConfig,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    by_ts, final_ts = build_book_view(notes)
    borrow_amount = config.initial_ltv * compute_buyback_price(config.notional, config)
    liquidated: set[str] = set()
    events: list[dict[str, Any]] = []
    daily_rows: list[dict[str, Any]] = []
    cumulative_buyback = 0.0
    cumulative_unwind = 0.0
    cumulative_buffer = 0.0

    for ts in sorted(by_ts):
        active = prepare_active_states(
            by_ts[ts], ts, final_ts, liquidated, borrow_amount, config
        )
        live_before = len(active)
        stress_day = returns_1d.get(ts, 0.0) <= config.stress_return_threshold
        todays_events: list[dict[str, Any]] = []
        if stress_day and active:
            active.sort(
                key=lambda state: (
                    state["current_ltv"],
                    -state["buyback_price_usdc"],
                    -state["current_capital_mark_usdc"],
                ),
                reverse=True,
            )
            count = max(1, math.ceil(config.forced_fraction * live_before))
            for state in active[:count]:
                todays_events.append(
                    make_event(
                        scenario="stress_concentration_coupled_book",
                        state=state,
                        borrow_amount=borrow_amount,
                        live_notes_before=live_before,
                        unwind_multiplier=config.stress_multiplier,
                        returns_1d=returns_1d,
                        vol_5d_ann=vol_5d_ann,
                        config=config,
                    )
                )

        for event in todays_events:
            liquidated.add(event["note_id"])
            cumulative_buyback += event["buyback_price_usdc"]
            cumulative_unwind += event["unwind_cost_usdc"]
            cumulative_buffer += event["buffer_usdc"]
        events.extend(todays_events)
        daily_rows.append(
            make_daily_row(
                scenario="stress_concentration_coupled_book",
                ts=ts,
                live_notes_before=live_before,
                todays_events=todays_events,
                returns_1d=returns_1d,
                vol_5d_ann=vol_5d_ann,
                cumulative_liquidations=len(events),
                cumulative_buyback_paid_usdc=cumulative_buyback,
                cumulative_unwind_cost_usdc=cumulative_unwind,
                cumulative_buffer_usdc=cumulative_buffer,
            )
        )

    return events, daily_rows


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        with path.open("w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["scenario"])
        return
    with path.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)


def write_markdown(
    path: Path,
    row_id: str,
    step_csv: Path,
    config: ScenarioConfig,
    total_notes: int,
    price_by_ts: dict[int, float],
    primary_summary: dict[str, Any],
    stress_summary: dict[str, Any],
) -> None:
    first_ts = min(price_by_ts)
    last_ts = max(price_by_ts)
    lines = [
        "# SOL Buyback Solvency",
        "",
        f"- Row: `{row_id}`",
        f"- Source step ledger: `{step_csv}`",
        f"- Replay method: `coupled book-level replay over live hedge states`",
        f"- Notes in ledger export: `{total_notes}`",
        f"- Underlying window: `{iso_utc(first_ts)}` to `{iso_utc(last_ts)}`",
        f"- Buyback formula overlay: `min(KI_level - 10%, current_capital_mark - 10%)`",
        f"- KI cap: `${config.buyback_cap_usdc:.2f}` on `${config.notional:.0f}` notional",
        f"- Lending trigger: initial LTV `{config.initial_ltv:.0%}`, liquidation LTV `{config.liquidation_ltv:.0%}`",
        f"- Stress liquidation test: `{config.forced_fraction:.0%}` of live notes on days with 24h return <= `{config.stress_return_threshold:.0%}`",
        "",
        "## Primary",
        "",
        f"- Liquidated notes: `{primary_summary['liquidated_notes']}` / `{total_notes}` (`{primary_summary['liquidated_fraction']:.2%}`)",
        f"- Buybacks always payable: `{primary_summary['buyback_possible_all']}`",
        f"- Failure count: `{primary_summary['failure_count']}`",
        f"- Min buffer: `${primary_summary['min_buffer_usdc']:.2f}`",
        f"- Min coverage ratio: `{primary_summary['min_coverage_ratio']:.4f}`",
        f"- Total buyback paid: `${primary_summary['total_buyback_paid_usdc']:.2f}`",
        f"- Total unwind cost: `${primary_summary['total_unwind_cost_usdc']:.2f}`",
        f"- Worst single day: `{primary_summary['worst_single_day_buyback_count']}` buybacks",
        f"- Worst 5d liquidation window: `{primary_summary['worst_5d_liquidations']}` notes, "
        f"`{primary_summary['worst_5d_window_start_utc']}` -> `{primary_summary['worst_5d_window_end_utc']}`",
        "",
        "## Stress Concentration",
        "",
        f"- Liquidated notes: `{stress_summary['liquidated_notes']}` / `{total_notes}` (`{stress_summary['liquidated_fraction']:.2%}`)",
        f"- Buybacks always payable: `{stress_summary['buyback_possible_all']}`",
        f"- Failure count: `{stress_summary['failure_count']}`",
        f"- Min buffer: `${stress_summary['min_buffer_usdc']:.2f}`",
        f"- Min coverage ratio: `{stress_summary['min_coverage_ratio']:.4f}`",
        f"- Total buyback paid: `${stress_summary['total_buyback_paid_usdc']:.2f}`",
        f"- Total unwind cost: `${stress_summary['total_unwind_cost_usdc']:.2f}`",
        f"- Worst single day: `{stress_summary['worst_single_day_buyback_count']}` buybacks",
        f"- Worst 5d liquidation window: `{stress_summary['worst_5d_liquidations']}` notes, "
        f"`{stress_summary['worst_5d_window_start_utc']}` -> `{stress_summary['worst_5d_window_end_utc']}`",
        "",
        "## Assumptions",
        "",
        "- The replay is coupled at the book level: buybacks fire inside the daily book loop and liquidated notes are removed immediately from all future days.",
        "- The production row uses separate sleeves and note-local hedge state, so removing a liquidated note does not alter surviving notes' hedge decisions.",
        "- `current_capital_mark = notional + hedge_cash + hedge_inventory * close - coupons_paid`",
        "- Unwind cost uses the production sqrt-impact curve from `halcyon_sol_autocall_quote::sol_swap_cost` with 10 bps base fee, coefficient 25, liquidity proxy $250k, and a 3x multiplier in stress.",
        "",
    ]
    path.write_text("\n".join(lines) + "\n")


def main() -> None:
    args = parse_args()
    config = ScenarioConfig(
        name=args.row_id,
        notional=args.notional,
        knock_in_barrier=args.knock_in_barrier,
        haircut_pct_of_notional=args.haircut_pct,
        initial_ltv=args.initial_ltv,
        liquidation_ltv=args.liquidation_ltv,
        base_fee_bps=args.base_fee_bps,
        slippage_coeff=args.slippage_coeff,
        liquidity_proxy_usdc=args.liquidity_proxy_usdc,
        keeper_bounty_usdc=args.keeper_bounty_usdc,
        stress_multiplier=args.stress_multiplier,
        stress_return_threshold=args.stress_return_threshold,
        stress_vol_threshold=args.stress_vol_threshold,
        forced_fraction=args.forced_fraction,
    )

    notes, price_by_ts = load_steps(args.step_csv, args.row_id)
    returns_1d, vol_5d_ann = build_market_context(price_by_ts)

    primary_events, primary_daily = run_primary_coupled_book_replay(
        notes, returns_1d, vol_5d_ann, config
    )
    stress_events, stress_daily = run_stress_coupled_book_replay(
        notes, returns_1d, vol_5d_ann, config
    )

    primary_summary = summarize_replay(
        "primary_ltv_coupled_book", primary_events, primary_daily, len(notes)
    )
    stress_summary = summarize_replay(
        "stress_concentration_coupled_book", stress_events, stress_daily, len(notes)
    )

    args.output_dir.mkdir(parents=True, exist_ok=True)
    summary_json = args.output_dir / "buyback_solvency_summary.json"
    summary_md = args.output_dir / "buyback_solvency_summary.md"
    primary_events_csv = args.output_dir / "buyback_solvency_primary_events.csv"
    primary_daily_csv = args.output_dir / "buyback_solvency_primary_daily.csv"
    stress_events_csv = args.output_dir / "buyback_solvency_stress_events.csv"
    stress_daily_csv = args.output_dir / "buyback_solvency_stress_daily.csv"

    write_csv(primary_events_csv, primary_events)
    write_csv(primary_daily_csv, primary_daily)
    write_csv(stress_events_csv, stress_events)
    write_csv(stress_daily_csv, stress_daily)

    payload = {
        "row_id": args.row_id,
        "source_step_csv": str(args.step_csv),
        "method": "coupled_book_level_replay_over_live_hedge_states",
        "assumptions": [
            "buybacks fire inside the daily book loop and liquidated notes are removed immediately from future days",
            "CURRENT_V1_HEDGED_BALANCED uses separate sleeves and note-local hedge state, so surviving notes keep their original hedge path after other notes liquidate",
            "current_capital_mark = notional + hedge_cash + hedge_inventory * close - coupons_paid",
            "buyback_price = min(KI_level - 10% notional, current_capital_mark - 10% notional)",
            "stress concentration liquidates 25% of live notes on days with 24h return <= -5%",
        ],
        "parameters": {
            "notional": config.notional,
            "knock_in_barrier": config.knock_in_barrier,
            "buyback_cap_usdc": config.buyback_cap_usdc,
            "haircut_usdc": config.haircut_usdc,
            "initial_ltv": config.initial_ltv,
            "liquidation_ltv": config.liquidation_ltv,
            "base_fee_bps": config.base_fee_bps,
            "slippage_coeff": config.slippage_coeff,
            "liquidity_proxy_usdc": config.liquidity_proxy_usdc,
            "keeper_bounty_usdc": config.keeper_bounty_usdc,
            "stress_multiplier": config.stress_multiplier,
            "stress_return_threshold": config.stress_return_threshold,
            "stress_vol_threshold": config.stress_vol_threshold,
            "forced_fraction": config.forced_fraction,
        },
        "dataset": {
            "notes": len(notes),
            "first_ts_ms": min(price_by_ts),
            "last_ts_ms": max(price_by_ts),
            "first_date_utc": iso_utc(min(price_by_ts)),
            "last_date_utc": iso_utc(max(price_by_ts)),
        },
        "primary": primary_summary,
        "stress_concentration": stress_summary,
    }
    summary_json.write_text(json.dumps(payload, indent=2) + "\n")
    write_markdown(
        summary_md,
        args.row_id,
        args.step_csv,
        config,
        len(notes),
        price_by_ts,
        primary_summary,
        stress_summary,
    )

    print(f"wrote {summary_json}")
    print(f"wrote {summary_md}")
    print(f"wrote {primary_events_csv}")
    print(f"wrote {primary_daily_csv}")
    print(f"wrote {stress_events_csv}")
    print(f"wrote {stress_daily_csv}")
    print()
    print(
        f"primary: {primary_summary['liquidated_notes']} liquidations, "
        f"failures={primary_summary['failure_count']}, "
        f"min_buffer={primary_summary['min_buffer_usdc']}"
    )
    print(
        f"stress: {stress_summary['liquidated_notes']} liquidations, "
        f"failures={stress_summary['failure_count']}, "
        f"min_buffer={stress_summary['min_buffer_usdc']}"
    )


if __name__ == "__main__":
    main()
