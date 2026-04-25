#!/usr/bin/env python3
from __future__ import annotations

import argparse
import concurrent.futures
import json
import math
import os
import sys
from collections import defaultdict
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import pandas as pd


CURRENT_REPO_ROOT = Path("/Users/dominic/colosseumfinal")
LEGACY_HEDGE_LAB_ROOT = Path("/Users/dominic/Colosseum/halcyon-hedge-lab")
LEGACY_SRC = LEGACY_HEDGE_LAB_ROOT / "src"
if str(LEGACY_SRC) not in sys.path:
    sys.path.insert(0, str(LEGACY_SRC))

from halcyon.config import read_yaml
from halcyon.data.multi_market_data import issue_indices, load_multi_market_frame
from halcyon.data.reserve_yield import load_reserve_yield_model
from halcyon.execution.multi_leg_issuance_gate import MultiLegIssueGateContext, MultiLegIssuanceGate
from halcyon.execution.rebalance_controller import RebalanceController, RebalanceRequest
from halcyon.hedges.common import trade_slippage_bps
from halcyon.hedges.solana_multi_wrapper_spot import SolanaMultiWrapperSpotStrategy
from halcyon.note.autocall import AutocallTerms, NoteRuntimeState, apply_observation, update_runtime_from_observation
from halcyon.pricing.multi_worst_of_state_pricer import MultiWorstOfPricingState, MultiWorstOfStatePricer
from halcyon.reports.spy_qqq_iwm_worst_of_hedged import (
    MultiPendingOrder,
    _append_proxy_hedge_columns,
    _band_units,
    _correlation_matrix,
    _hedge_legs,
    _latency_days,
    _target_units,
)


DEFAULT_OUTPUT_DIR = CURRENT_REPO_ROOT / "research" / "flagship_buyback_outputs"
DEFAULT_STEP_LEDGER_CSV = DEFAULT_OUTPUT_DIR / "flagship_step_ledger.csv"
DEFAULT_SOURCE_SUMMARY_CSV = (
    LEGACY_HEDGE_LAB_ROOT
    / "outputs"
    / "spy_qqq_iwm_factor_model_quarterly_recal_q65_cap500_daily"
    / "summary.csv"
)
DEFAULT_BASE_CONFIG = LEGACY_HEDGE_LAB_ROOT / "configs" / "spy_qqq_iwm_worst_of_sweep_proxy.yaml"
DEFAULT_FACTOR_MODEL_DIR = LEGACY_HEDGE_LAB_ROOT / "output" / "factor_model" / "quarterly"

_WORKER_CFG: dict[str, Any] | None = None
_WORKER_FRAME: pd.DataFrame | None = None
_WORKER_ASSET_KEYS: list[str] | None = None
_WORKER_PRICER: MultiWorstOfStatePricer | None = None
_WORKER_CONTROLLER: RebalanceController | None = None
_WORKER_RESERVE_YIELD_MODEL: Any | None = None
_WORKER_ISSUANCE_GATE: MultiLegIssuanceGate | None = None


@dataclass
class ScenarioConfig:
    name: str
    notional: float
    knock_in_barrier: float
    haircut_pct_of_notional: float = 0.10
    initial_ltv: float = 0.70
    liquidation_ltv: float = 0.85
    stress_multiplier: float = 3.0
    stress_return_threshold: float = -0.05
    stress_vol_threshold: float = 1.0
    forced_fraction: float = 0.25

    @property
    def haircut_usdc(self) -> float:
        return self.notional * self.haircut_pct_of_notional

    @property
    def buyback_cap_usdc(self) -> float:
        return self.notional * self.knock_in_barrier - self.haircut_usdc


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run a mechanism-active flagship buyback solvency replay using the "
            "current q65/cap500 daily-issuance worst-of SPY/QQQ/IWM backtest."
        )
    )
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--step-ledger-csv", type=Path, default=DEFAULT_STEP_LEDGER_CSV)
    parser.add_argument("--reuse-ledger", action="store_true")
    parser.add_argument("--max-issued", type=int, default=0, help="Limit issued notes for smoke testing.")
    parser.add_argument("--haircut-pct", type=float, default=0.10)
    parser.add_argument("--initial-ltv", type=float, default=0.70)
    parser.add_argument("--liquidation-ltv", type=float, default=0.85)
    parser.add_argument("--stress-multiplier", type=float, default=3.0)
    parser.add_argument("--stress-return-threshold", type=float, default=-0.05)
    parser.add_argument("--stress-vol-threshold", type=float, default=1.0)
    parser.add_argument("--forced-fraction", type=float, default=0.25)
    parser.add_argument("--workers", type=int, default=max(1, min(8, os.cpu_count() or 1)))
    parser.add_argument("--chunk-size", type=int, default=25)
    return parser.parse_args()


def build_cfg() -> dict[str, Any]:
    base = read_yaml(DEFAULT_BASE_CONFIG)
    cfg = dict(base)
    cfg["note"] = dict(base["note"])
    cfg["note"]["coupon_barrier"] = 1.0
    cfg["note"]["coupon_interval_days"] = 21
    cfg["note"]["knock_in_barrier"] = 0.80
    cfg["note"]["tenor_trading_days"] = 378
    cfg["note"]["observation_interval_days"] = 63
    cfg["note"]["autocall_barriers"] = [1.0] * 6
    cfg["note"]["coupon_policy"] = "memory"
    cfg["note"]["issuance_fee_bps"] = 100
    cfg["note"]["reserve_yield_apr"] = 0.04
    cfg["sweep"] = dict(base["sweep"])
    cfg["sweep"]["knock_in_barriers"] = [0.80]
    cfg["sweep"]["quote_fractions"] = [0.65]
    cfg["sweep"]["issuer_margin_bps"] = [100]
    cfg["sweep"]["baskets"] = [b for b in base["sweep"]["baskets"] if b["name"] == "spy_qqq_iwm_proxy"]
    data_cfg = dict(base.get("data", {}))
    basket_assets = cfg["sweep"]["baskets"][0]["assets"]
    data_cfg["assets"] = basket_assets
    cfg["data"] = data_cfg
    cfg["pricing"] = dict(base["pricing"])
    cfg["pricing"].pop("nig", None)
    cfg["pricing"]["factor_model"] = {
        "enabled": True,
        "calibration_dir": str(DEFAULT_FACTOR_MODEL_DIR),
    }
    cfg["pricing"]["fair_coupon_mc_paths"] = 3000
    cfg["pricing"]["delta_mc_paths"] = 500
    cfg["issuance_gate"] = {
        "enabled": True,
        "min_fair_coupon_rate_per_observation": 0.0050,
        "max_fair_coupon_rate_per_observation": 0.05,
    }
    cfg["execution"] = dict(cfg.get("execution", {}))
    cfg["execution"]["policy"] = "delta_band_raw"
    cfg["execution"]["delta_band_fraction_of_notional"] = 0.005
    cfg["execution"]["daily_check"] = True
    cfg["simulation"] = dict(cfg.get("simulation", {}))
    cfg["simulation"]["issuance_stride_days"] = 1
    return cfg


def _basket_issue_terms(
    config: dict[str, Any],
    coupon_rate: float,
    knock_in_barrier: float,
    issuance_fee_bps: float,
    basket_name: str,
) -> AutocallTerms:
    note_cfg = dict(config.get("note", {}))
    note_cfg["coupon_rate_per_observation"] = coupon_rate
    note_cfg["knock_in_barrier"] = knock_in_barrier
    note_cfg["issuance_fee_bps"] = issuance_fee_bps
    note_cfg.pop("fair_coupon_rate_per_observation", None)
    note_cfg["underlying"] = basket_name
    return AutocallTerms.from_config({"note": note_cfg, "run_name": basket_name})


def iso_utc(ts: pd.Timestamp) -> str:
    return ts.strftime("%Y-%m-%d")


def compute_buyback_price(current_nav: float, config: ScenarioConfig) -> float:
    return max(0.0, min(config.buyback_cap_usdc, current_nav - config.haircut_usdc))


def adverse_unwind_value_and_cost(
    row: dict[str, Any],
    hedge_legs: list[str],
    config: dict[str, Any],
    multiplier: float,
) -> tuple[float, float]:
    oracle_cfg = config.get("data", {}).get("oracle", {})
    conf_bps = float(oracle_cfg.get("confidence_bps", 5.0))
    cost_cfg = config.get("costs", {}).get("jupiter_swap_cost", {})
    keeper_cfg = config.get("costs", {}).get("keeper_cost", {})
    fee_bps = float(cost_cfg.get("fee_bps", 4.0))
    a_bps = float(cost_cfg.get("slippage_a_bps", 2.0))
    b_bps = float(cost_cfg.get("slippage_b_bps", 18.0))
    daily_liquidity = float(cost_cfg.get("daily_liquidity_usd", 50_000_000.0))
    keeper_cost = float(keeper_cfg.get("bounty_usd", 0.50)) + float(keeper_cfg.get("tx_usd", 0.10))

    total_cashflow = float(row["hedge_cash_usdc"])
    total_cost = 0.0
    for leg in hedge_legs:
        units = float(row[f"{leg}_units"])
        if abs(units) < 1e-12:
            continue
        wrap_close = float(row[f"{leg}_wrap_close"])
        if units >= 0.0:
            adverse_price = wrap_close * (1.0 - conf_bps / 10_000.0)
        else:
            adverse_price = wrap_close * (1.0 + conf_bps / 10_000.0)
        trade_notional = abs(units) * adverse_price
        slippage_bps = trade_slippage_bps(trade_notional, daily_liquidity, a_bps, b_bps)
        fee_cost = trade_notional * fee_bps / 10_000.0
        slippage_cost = trade_notional * slippage_bps * multiplier / 10_000.0
        leg_keeper = keeper_cost * multiplier
        total_cashflow += units * adverse_price
        total_cost += fee_cost + slippage_cost + leg_keeper
    return total_cashflow - total_cost, total_cost


def build_market_context(frame: pd.DataFrame, asset_keys: list[str]) -> tuple[dict[str, float], dict[str, float]]:
    returns: dict[str, float] = {}
    vols: dict[str, float] = {}
    for idx in range(len(frame)):
        date = iso_utc(pd.Timestamp(frame.iloc[idx]["date"]))
        if idx == 0:
            returns[date] = 0.0
        else:
            returns[date] = min(
                float(frame.iloc[idx][f"{key}_close"] / frame.iloc[idx - 1][f"{key}_close"] - 1.0)
                for key in asset_keys
            )
        if idx < 5:
            vols[date] = 0.0
        else:
            rolling = []
            for key in asset_keys:
                rets = [
                    math.log(float(frame.iloc[j][f"{key}_close"] / frame.iloc[j - 1][f"{key}_close"]))
                    for j in range(idx - 4, idx + 1)
                ]
                avg = sum(rets) / len(rets)
                var = sum((ret - avg) ** 2 for ret in rets) / len(rets)
                rolling.append(math.sqrt(var) * math.sqrt(252.0))
            vols[date] = max(rolling)
    return returns, vols


def simulate_note_daily_ledger(
    config: dict[str, Any],
    frame: pd.DataFrame,
    asset_keys: list[str],
    start_idx: int,
    terms: AutocallTerms,
    quote_fraction: float,
    fair_coupon_rate: float,
    pricer: MultiWorstOfStatePricer,
    controller: RebalanceController,
    reserve_yield_model: Any,
    seed_offset: int,
    note_id: str,
) -> list[dict[str, Any]]:
    runtime = NoteRuntimeState()
    observation_days = set(terms.observation_days())
    coupon_day_set = set(terms.coupon_days())
    all_event_days = set(terms.all_event_days())
    issue_row = frame.iloc[start_idx]
    issue_date = pd.Timestamp(issue_row["date"])
    issue_prices = {key: float(issue_row[f"{key}_close"]) for key in asset_keys}

    capital_cfg = config.get("capital_stack", {})
    junior_ratio = float(capital_cfg.get("junior_first_loss_ratio", 0.10))
    junior_capital = terms.notional * junior_ratio
    explicit_fee = terms.notional * terms.issuance_fee_bps / 10_000.0
    starting_cash = terms.notional + junior_capital + explicit_fee

    hedge_legs = _hedge_legs(config, asset_keys)
    strategy = SolanaMultiWrapperSpotStrategy(config=config, legs=tuple(hedge_legs), starting_cash=starting_cash)
    order_latency = _latency_days(config)
    pending_orders: list[MultiPendingOrder] = []
    last_rebalance_day: dict[str, int | None] = {key: None for key in hedge_legs}
    prev_accrual_date = issue_date
    note_paid = False

    annual_vols = tuple(float(issue_row[f"{key}_ewma_vol"]) for key in asset_keys)
    corr_matrix = _correlation_matrix(issue_row, asset_keys)
    init_price = pricer.price(
        terms,
        MultiWorstOfPricingState(tuple(1.0 for _ in asset_keys), False, 0, 0),
        annual_vols=annual_vols,
        corr_matrix=corr_matrix,
        seed_offset=seed_offset,
    )
    initial_targets = _target_units(config, asset_keys, issue_row, issue_prices, init_price.deltas)
    for key in hedge_legs:
        pending_orders.append(
            MultiPendingOrder(min(order_latency, terms.tenor_trading_days + 1), key, initial_targets[key], "initial_hedge")
        )

    ledger_rows: list[dict[str, Any]] = []
    for rel_day in range(1, terms.tenor_trading_days + order_latency + 2):
        idx = start_idx + rel_day
        if idx >= len(frame):
            break
        row = frame.iloc[idx]
        date = pd.Timestamp(row["date"])
        market_open = date.weekday() < 5
        stale = False
        if idx > 0:
            prev_date = pd.Timestamp(frame.iloc[idx - 1]["date"])
            stale = (date - prev_date).days > int(config.get("data", {}).get("oracle", {}).get("max_staleness_days", 1)) + 3
        conf_bps = float(config.get("data", {}).get("oracle", {}).get("confidence_bps", 5.0))

        due_orders = [order for order in pending_orders if order.execute_day == rel_day]
        pending_orders = [order for order in pending_orders if order.execute_day != rel_day]
        if due_orders:
            if stale or not market_open:
                for order in due_orders:
                    pending_orders.append(
                        MultiPendingOrder(rel_day + 1, order.leg, order.units, f"{order.reason}_deferred")
                    )
                    strategy.stats.execution_failures += 1
                    strategy.stats.retries += 1
            else:
                for order in due_orders:
                    strategy.execute_trade(order.leg, order.units, float(row[f"{order.leg}_wrap_open"]), config)
                    last_rebalance_day[order.leg] = rel_day

        if rel_day > 1:
            prev_row = frame.iloc[idx - 1]
            for key in hedge_legs:
                strategy.mark_basis(
                    key,
                    float(prev_row[f"{key}_close"]),
                    float(row[f"{key}_close"]),
                    float(prev_row[f"{key}_wrap_close"]),
                    float(row[f"{key}_wrap_close"]),
                )

        reserve_interval_return = reserve_yield_model.interval_return(prev_accrual_date, date)
        strategy.accrue_reserve_yield(reserve_interval_return)
        prev_accrual_date = date

        close_ratios = {key: float(row[f"{key}_close"] / max(issue_prices[key], 1e-8)) for key in asset_keys}
        low_ratios = {key: float(row[f"{key}_low"] / max(issue_prices[key], 1e-8)) for key in asset_keys}
        worst_close_ratio = min(close_ratios.values())
        worst_low_ratio = min(low_ratios.values())
        if not note_paid:
            runtime.knock_in_triggered = (
                runtime.knock_in_triggered
                or worst_low_ratio < terms.knock_in_barrier
                or worst_close_ratio < terms.knock_in_barrier
            )

        forced_check = False
        is_event_day = rel_day in all_event_days
        is_coupon_only = rel_day in coupon_day_set and rel_day not in observation_days
        is_autocall_day = rel_day in observation_days
        is_maturity = rel_day == terms.tenor_trading_days

        if is_event_day and not runtime.autocalled and not runtime.matured:
            forced_check = True
            if is_coupon_only and not is_maturity:
                coupon_due = worst_close_ratio >= terms.coupon_barrier
                if terms.coupon_policy == "memory":
                    if coupon_due:
                        cpn = terms.notional * terms.coupon_rate_per_observation * (runtime.missed_coupon_observations + 1)
                        strategy.cash -= cpn
                        runtime.missed_coupon_observations = 0
                    else:
                        runtime.missed_coupon_observations += 1
                elif terms.coupon_policy == "cash":
                    if coupon_due:
                        strategy.cash -= terms.notional * terms.coupon_rate_per_observation
                elif terms.coupon_policy == "accrual":
                    if coupon_due:
                        runtime.accrued_coupon_liability += terms.notional * terms.coupon_rate_per_observation
            elif is_autocall_day or is_maturity:
                observation = apply_observation(
                    terms=terms,
                    runtime=runtime,
                    reference_ratio=worst_close_ratio,
                    intraday_low_ratio=worst_low_ratio,
                    is_maturity=is_maturity,
                )
                update_runtime_from_observation(runtime, observation)
                strategy.cash -= observation.coupon_paid_now

                if observation.autocalled or observation.matured:
                    redemption_cash = observation.redemption_due + runtime.accrued_coupon_liability
                    strategy.cash -= redemption_cash
                    runtime.accrued_coupon_liability = 0.0
                    note_paid = True
                    for key in hedge_legs:
                        current_units = strategy.current_delta_units(
                            key,
                            float(row[f"{key}_wrap_close"] / max(row[f"{key}_close"], 1e-8)),
                        )
                        pending_orders.append(MultiPendingOrder(rel_day + order_latency, key, -current_units, "terminal_unwind"))

        if note_paid and not pending_orders and all(abs(strategy.units[key]) < 1e-8 for key in hedge_legs):
            break

        should_check = forced_check or (
            controller.force_daily_check
            and rel_day <= terms.tenor_trading_days
            and rel_day % max(1, controller.check_every_n_days) == 0
        )
        priced = None
        if should_check:
            if runtime.autocalled or runtime.matured:
                target_units = {key: 0.0 for key in hedge_legs}
            else:
                priced = pricer.price(
                    terms,
                    MultiWorstOfPricingState(
                        tuple(close_ratios[key] for key in asset_keys),
                        runtime.knock_in_triggered,
                        rel_day,
                        runtime.missed_coupon_observations,
                    ),
                    annual_vols=tuple(float(row[f"{key}_ewma_vol"]) for key in asset_keys),
                    corr_matrix=_correlation_matrix(row, asset_keys),
                    seed_offset=seed_offset + rel_day,
                )
                target_units = _target_units(config, asset_keys, issue_row, issue_prices, priced.deltas)

            for key in hedge_legs:
                pending_for_leg = len([order for order in pending_orders if order.leg == key])
                decision = controller.evaluate(
                    RebalanceRequest(
                        current_day=rel_day,
                        target_delta_units=target_units[key],
                        current_hedge_delta_units=strategy.current_delta_units(
                            key,
                            float(row[f"{key}_wrap_close"] / max(row[f"{key}_close"], 1e-8)),
                        ),
                        band_units=_band_units(config, float(row[f"{key}_close"]), len(asset_keys)),
                        min_trade_units=controller.min_trade_units,
                        max_trade_units=controller.max_trade_units,
                        cooldown_days=controller.cooldown_days,
                        last_rebalance_day=last_rebalance_day[key],
                        market_hours=market_open,
                        stale_data=stale,
                        forced_check=forced_check,
                        max_actions_per_day=controller.max_actions_per_day,
                        actions_taken_today=0,
                        reference_ratio=worst_close_ratio,
                        is_observation_day=rel_day in observation_days,
                        days_to_next_observation=min(
                            [day - rel_day for day in observation_days if day >= rel_day],
                            default=None,
                        ),
                        pending_order_count=pending_for_leg,
                        oracle_confidence_bps=conf_bps,
                    )
                )
                if decision.should_trade:
                    pending_orders.append(MultiPendingOrder(rel_day + order_latency, key, decision.trade_units, decision.reason))

        if note_paid or runtime.autocalled or runtime.matured:
            continue

        if priced is None:
            priced = pricer.price(
                terms,
                MultiWorstOfPricingState(
                    tuple(close_ratios[key] for key in asset_keys),
                    runtime.knock_in_triggered,
                    rel_day,
                    runtime.missed_coupon_observations,
                ),
                annual_vols=tuple(float(row[f"{key}_ewma_vol"]) for key in asset_keys),
                corr_matrix=_correlation_matrix(row, asset_keys),
                seed_offset=seed_offset + rel_day,
            )

        wrapper_closes = {key: float(row[f"{key}_wrap_close"]) for key in hedge_legs}
        hedge_inventory_mark = sum(strategy.units[key] * wrapper_closes[key] for key in hedge_legs)
        assets = strategy.total_assets(wrapper_closes)
        note_liability = float(priced.pv + runtime.accrued_coupon_liability)

        record: dict[str, Any] = {
            "note_id": note_id,
            "entry_index": int(start_idx),
            "issue_date": iso_utc(issue_date),
            "date": iso_utc(date),
            "step_day": int(rel_day),
            "quote_fraction": float(quote_fraction),
            "fair_coupon_rate_per_observation": float(fair_coupon_rate),
            "quoted_coupon_rate_per_observation": float(terms.coupon_rate_per_observation),
            "worst_close_ratio": float(worst_close_ratio),
            "worst_low_ratio": float(worst_low_ratio),
            "note_liability_usdc": note_liability,
            "current_assets_usdc": float(assets),
            "hedge_cash_usdc": float(strategy.cash),
            "hedge_inventory_mark_usdc": float(hedge_inventory_mark),
            "accrued_coupon_liability_usdc": float(runtime.accrued_coupon_liability),
            "knock_in_triggered": bool(runtime.knock_in_triggered),
        }
        for key in hedge_legs:
            record[f"{key}_units"] = float(strategy.units[key])
            record[f"{key}_wrap_close"] = float(row[f"{key}_wrap_close"])
            record[f"{key}_close"] = float(row[f"{key}_close"])
        ledger_rows.append(record)

    return ledger_rows


def generate_step_ledger(args: argparse.Namespace) -> tuple[pd.DataFrame, dict[str, Any]]:
    cfg = build_cfg()
    frame, asset_keys = load_multi_market_frame(cfg, LEGACY_HEDGE_LAB_ROOT)
    frame = _append_proxy_hedge_columns(frame, cfg, asset_keys)

    quote_fraction = 0.65
    knock_in_barrier = 0.80
    issuer_margin_bps = 100.0
    indices = issue_indices(frame, cfg)
    rows: list[dict[str, Any]] = []
    issued_notes = 0
    candidate_windows = 0

    if args.max_issued:
        _init_worker()
        for window_num, start_idx in enumerate(indices, start=1):
            chunk_rows, chunk_candidate, chunk_issued = _process_windows_chunk([start_idx])
            candidate_windows += chunk_candidate
            issued_notes += chunk_issued
            rows.extend(chunk_rows)
            if window_num % 20 == 0:
                print(f"flagship buyback ledger: window {window_num}/{len(indices)}", flush=True)
            if issued_notes >= args.max_issued:
                break
    else:
        chunks = [indices[idx : idx + args.chunk_size] for idx in range(0, len(indices), args.chunk_size)]
        with concurrent.futures.ProcessPoolExecutor(
            max_workers=max(1, args.workers),
            initializer=_init_worker,
        ) as executor:
            futures = {executor.submit(_process_windows_chunk, chunk): chunk for chunk in chunks}
            completed = 0
            last_logged = 0
            for future in concurrent.futures.as_completed(futures):
                chunk_rows, chunk_candidate, chunk_issued = future.result()
                completed += len(futures[future])
                candidate_windows += chunk_candidate
                issued_notes += chunk_issued
                rows.extend(chunk_rows)
                if completed - last_logged >= 20 or completed == len(indices):
                    print(f"flagship buyback ledger: window {completed}/{len(indices)}", flush=True)
                    last_logged = completed

    ledger = pd.DataFrame(rows)
    if not ledger.empty:
        ledger = ledger.sort_values(["date", "note_id", "step_day"]).reset_index(drop=True)
    metadata = {
        "candidate_windows": candidate_windows,
        "issued_windows": issued_notes,
        "asset_keys": asset_keys,
        "config_name": "spy_qqq_iwm_factor_model_quarterly_recal_q65_cap500_daily",
        "legacy_root": str(LEGACY_HEDGE_LAB_ROOT),
        "source_summary_csv": str(DEFAULT_SOURCE_SUMMARY_CSV),
        "max_issued": args.max_issued,
    }
    return ledger, metadata


def _init_worker() -> None:
    global _WORKER_CFG, _WORKER_FRAME, _WORKER_ASSET_KEYS, _WORKER_PRICER
    global _WORKER_CONTROLLER, _WORKER_RESERVE_YIELD_MODEL, _WORKER_ISSUANCE_GATE
    if _WORKER_CFG is not None:
        return
    _WORKER_CFG = build_cfg()
    frame, asset_keys = load_multi_market_frame(_WORKER_CFG, LEGACY_HEDGE_LAB_ROOT)
    _WORKER_FRAME = _append_proxy_hedge_columns(frame, _WORKER_CFG, asset_keys)
    _WORKER_ASSET_KEYS = asset_keys
    _WORKER_PRICER = MultiWorstOfStatePricer(_WORKER_CFG)
    _WORKER_CONTROLLER = RebalanceController(_WORKER_CFG)
    _WORKER_RESERVE_YIELD_MODEL = load_reserve_yield_model(_WORKER_CFG, LEGACY_HEDGE_LAB_ROOT)
    _WORKER_ISSUANCE_GATE = MultiLegIssuanceGate(_WORKER_CFG)


def _process_windows_chunk(start_indices: list[int]) -> tuple[list[dict[str, Any]], int, int]:
    assert _WORKER_CFG is not None
    assert _WORKER_FRAME is not None
    assert _WORKER_ASSET_KEYS is not None
    assert _WORKER_PRICER is not None
    assert _WORKER_CONTROLLER is not None
    assert _WORKER_RESERVE_YIELD_MODEL is not None
    assert _WORKER_ISSUANCE_GATE is not None

    cfg = _WORKER_CFG
    frame = _WORKER_FRAME
    asset_keys = _WORKER_ASSET_KEYS
    pricer = _WORKER_PRICER
    controller = _WORKER_CONTROLLER
    reserve_yield_model = _WORKER_RESERVE_YIELD_MODEL
    issuance_gate = _WORKER_ISSUANCE_GATE

    quote_fraction = 0.65
    knock_in_barrier = 0.80
    issuer_margin_bps = 100.0
    basket_name = "spy_qqq_iwm_proxy"

    rows: list[dict[str, Any]] = []
    issued_notes = 0
    for start_idx in start_indices:
        issue_row = frame.iloc[start_idx]
        issue_date = iso_utc(pd.Timestamp(issue_row["date"]))
        if hasattr(pricer, "set_active_date"):
            pricer.set_active_date(issue_date)
        corr_matrix = _correlation_matrix(issue_row, asset_keys)
        annual_vols = tuple(float(issue_row[f"{key}_ewma_vol"]) for key in asset_keys)

        base_terms = _basket_issue_terms(cfg, 0.0, knock_in_barrier, 0.0, basket_name)
        fair_coupon, expected_life = pricer.fair_coupon_rate(
            base_terms,
            annual_vols,
            corr_matrix,
            seed_offset=int(start_idx + 1000 * knock_in_barrier),
        )
        gate_decision = issuance_gate.evaluate(
            MultiLegIssueGateContext(
                ewma_vols={key: float(issue_row[f"{key}_ewma_vol"]) for key in asset_keys},
                modeled_basis_bps={key: float(issue_row.get(f"{key}_modeled_basis_bps", 0.0)) for key in asset_keys},
                worst_drawdown_63d=float(issue_row["worst_drawdown_63d"]),
                worst_drawdown_126d=float(issue_row["worst_drawdown_126d"]),
                fair_coupon_rate_per_observation=float(fair_coupon),
                expected_life_trading_days=float(expected_life),
            )
        )
        if not gate_decision.should_issue:
            continue

        issued_notes += 1
        terms = _basket_issue_terms(cfg, fair_coupon * quote_fraction, knock_in_barrier, issuer_margin_bps, basket_name)
        run_cfg = dict(cfg)
        run_cfg["run_name"] = f"{basket_name}_ki80_q65_m100"
        note_rows = simulate_note_daily_ledger(
            config=run_cfg,
            frame=frame,
            asset_keys=asset_keys,
            start_idx=start_idx,
            terms=terms,
            quote_fraction=quote_fraction,
            fair_coupon_rate=fair_coupon,
            pricer=pricer,
            controller=controller,
            reserve_yield_model=reserve_yield_model,
            seed_offset=int(start_idx + 1000 * quote_fraction + 10_000 * knock_in_barrier + 100_000 * issuer_margin_bps),
            note_id=issue_date,
        )
        rows.extend(note_rows)

    return rows, len(start_indices), issued_notes


def load_step_ledger(path: Path) -> pd.DataFrame:
    ledger = pd.read_csv(path)
    bool_cols = ["knock_in_triggered"]
    for col in bool_cols:
        if col in ledger.columns:
            ledger[col] = ledger[col].astype(bool)
    return ledger


def primary_replay(
    ledger: pd.DataFrame,
    scenario: ScenarioConfig,
    cfg: dict[str, Any],
    hedge_legs: list[str],
    market_returns: dict[str, float],
    market_vols: dict[str, float],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    borrow_amount = scenario.initial_ltv * compute_buyback_price(scenario.notional, scenario)
    liquidated: set[str] = set()
    events: list[dict[str, Any]] = []
    daily_rows: list[dict[str, Any]] = []
    cumulative_buyback = 0.0
    cumulative_unwind = 0.0
    cumulative_buffer = 0.0

    for date, day_frame in ledger.groupby("date", sort=True):
        active = day_frame[~day_frame["note_id"].isin(liquidated)].copy()
        live_before = len(active)
        todays_events: list[dict[str, Any]] = []
        for row in active.to_dict(orient="records"):
            current_nav = float(row["note_liability_usdc"])
            buyback_price = compute_buyback_price(current_nav, scenario)
            current_ltv = borrow_amount / buyback_price if buyback_price > 0.0 else math.inf
            if current_ltv < scenario.liquidation_ltv:
                continue
            available_after_unwind, unwind_cost = adverse_unwind_value_and_cost(row, hedge_legs, cfg, 1.0)
            buffer = available_after_unwind - buyback_price
            coverage = available_after_unwind / buyback_price if buyback_price > 0.0 else math.inf
            event = {
                "scenario": "primary_lending_liquidation",
                "note_id": row["note_id"],
                "entry_index": int(row["entry_index"]),
                "issue_date": row["issue_date"],
                "event_date_utc": date,
                "step_day": int(row["step_day"]),
                "live_notes_before": live_before,
                "worst_close_ratio": float(row["worst_close_ratio"]),
                "worst_low_ratio": float(row["worst_low_ratio"]),
                "knock_in_triggered": bool(row["knock_in_triggered"]),
                "note_liability_usdc": current_nav,
                "buyback_price_usdc": buyback_price,
                "borrow_amount_usdc": borrow_amount,
                "current_ltv": current_ltv,
                "current_assets_usdc": float(row["current_assets_usdc"]),
                "hedge_cash_usdc": float(row["hedge_cash_usdc"]),
                "hedge_inventory_mark_usdc": float(row["hedge_inventory_mark_usdc"]),
                "unwind_cost_usdc": unwind_cost,
                "available_after_unwind_usdc": available_after_unwind,
                "buffer_usdc": buffer,
                "coverage_ratio": coverage,
                "return_1d": market_returns.get(date, 0.0),
                "vol_5d_ann": market_vols.get(date, 0.0),
                "stress_day": market_returns.get(date, 0.0) <= scenario.stress_return_threshold
                or market_vols.get(date, 0.0) >= scenario.stress_vol_threshold,
                "trigger_reason": "ltv_threshold",
                "unwind_multiplier": 1.0,
            }
            for leg in hedge_legs:
                event[f"{leg}_units"] = float(row[f"{leg}_units"])
            todays_events.append(event)

        for event in todays_events:
            liquidated.add(event["note_id"])
            cumulative_buyback += event["buyback_price_usdc"]
            cumulative_unwind += event["unwind_cost_usdc"]
            cumulative_buffer += event["buffer_usdc"]
        events.extend(todays_events)

        daily_rows.append(
            {
                "scenario": "primary_lending_liquidation",
                "date_utc": date,
                "live_notes_before": live_before,
                "liquidated_today": len(todays_events),
                "live_notes_after": max(0, live_before - len(todays_events)),
                "stress_day": market_returns.get(date, 0.0) <= scenario.stress_return_threshold
                or market_vols.get(date, 0.0) >= scenario.stress_vol_threshold,
                "return_1d": market_returns.get(date, 0.0),
                "vol_5d_ann": market_vols.get(date, 0.0),
                "daily_buyback_paid_usdc": sum(event["buyback_price_usdc"] for event in todays_events),
                "daily_unwind_cost_usdc": sum(event["unwind_cost_usdc"] for event in todays_events),
                "daily_buffer_usdc": sum(event["buffer_usdc"] for event in todays_events),
                "min_event_buffer_usdc": min((event["buffer_usdc"] for event in todays_events), default=None),
                "max_event_ltv": max((event["current_ltv"] for event in todays_events), default=None),
                "cumulative_liquidations": len(events),
                "cumulative_buyback_paid_usdc": cumulative_buyback,
                "cumulative_unwind_cost_usdc": cumulative_unwind,
                "cumulative_buffer_usdc": cumulative_buffer,
            }
        )

    return events, daily_rows


def stress_replay(
    ledger: pd.DataFrame,
    scenario: ScenarioConfig,
    cfg: dict[str, Any],
    hedge_legs: list[str],
    market_returns: dict[str, float],
    market_vols: dict[str, float],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    liquidated: set[str] = set()
    events: list[dict[str, Any]] = []
    daily_rows: list[dict[str, Any]] = []
    cumulative_buyback = 0.0
    cumulative_unwind = 0.0
    cumulative_buffer = 0.0

    for date, day_frame in ledger.groupby("date", sort=True):
        active = day_frame[~day_frame["note_id"].isin(liquidated)].copy()
        live_before = len(active)
        return_1d = market_returns.get(date, 0.0)
        vol_5d_ann = market_vols.get(date, 0.0)
        stress_day = return_1d <= scenario.stress_return_threshold or vol_5d_ann >= scenario.stress_vol_threshold
        todays_events: list[dict[str, Any]] = []
        if stress_day and live_before > 0:
            borrow_amount = scenario.initial_ltv * compute_buyback_price(scenario.notional, scenario)
            ranked: list[tuple[float, float, dict[str, Any]]] = []
            for row in active.to_dict(orient="records"):
                current_nav = float(row["note_liability_usdc"])
                buyback_price = compute_buyback_price(current_nav, scenario)
                current_ltv = borrow_amount / buyback_price if buyback_price > 0.0 else math.inf
                ranked.append((current_ltv, buyback_price, row))
            ranked.sort(key=lambda item: (item[0], item[1]), reverse=True)
            count = max(1, math.ceil(scenario.forced_fraction * live_before))
            for _, _, row in ranked[:count]:
                current_nav = float(row["note_liability_usdc"])
                buyback_price = compute_buyback_price(current_nav, scenario)
                available_after_unwind, unwind_cost = adverse_unwind_value_and_cost(
                    row, hedge_legs, cfg, scenario.stress_multiplier
                )
                buffer = available_after_unwind - buyback_price
                coverage = available_after_unwind / buyback_price if buyback_price > 0.0 else math.inf
                event = {
                    "scenario": "stress_concentration_coupled_book",
                    "note_id": row["note_id"],
                    "entry_index": int(row["entry_index"]),
                    "issue_date": row["issue_date"],
                    "event_date_utc": date,
                    "step_day": int(row["step_day"]),
                    "live_notes_before": live_before,
                    "worst_close_ratio": float(row["worst_close_ratio"]),
                    "worst_low_ratio": float(row["worst_low_ratio"]),
                    "knock_in_triggered": bool(row["knock_in_triggered"]),
                    "note_liability_usdc": current_nav,
                    "buyback_price_usdc": buyback_price,
                    "borrow_amount_usdc": borrow_amount,
                    "current_ltv": borrow_amount / buyback_price if buyback_price > 0.0 else math.inf,
                    "current_assets_usdc": float(row["current_assets_usdc"]),
                    "hedge_cash_usdc": float(row["hedge_cash_usdc"]),
                    "hedge_inventory_mark_usdc": float(row["hedge_inventory_mark_usdc"]),
                    "unwind_cost_usdc": unwind_cost,
                    "available_after_unwind_usdc": available_after_unwind,
                    "buffer_usdc": buffer,
                    "coverage_ratio": coverage,
                    "return_1d": return_1d,
                    "vol_5d_ann": vol_5d_ann,
                    "stress_day": True,
                    "trigger_reason": "stress_day_forced_fraction",
                    "unwind_multiplier": scenario.stress_multiplier,
                }
                for leg in hedge_legs:
                    event[f"{leg}_units"] = float(row[f"{leg}_units"])
                todays_events.append(event)

        for event in todays_events:
            liquidated.add(event["note_id"])
            cumulative_buyback += event["buyback_price_usdc"]
            cumulative_unwind += event["unwind_cost_usdc"]
            cumulative_buffer += event["buffer_usdc"]
        events.extend(todays_events)

        daily_rows.append(
            {
                "scenario": "stress_concentration_coupled_book",
                "date_utc": date,
                "live_notes_before": live_before,
                "liquidated_today": len(todays_events),
                "live_notes_after": max(0, live_before - len(todays_events)),
                "stress_day": stress_day,
                "return_1d": return_1d,
                "vol_5d_ann": vol_5d_ann,
                "daily_buyback_paid_usdc": sum(event["buyback_price_usdc"] for event in todays_events),
                "daily_unwind_cost_usdc": sum(event["unwind_cost_usdc"] for event in todays_events),
                "daily_buffer_usdc": sum(event["buffer_usdc"] for event in todays_events),
                "min_event_buffer_usdc": min((event["buffer_usdc"] for event in todays_events), default=None),
                "max_event_ltv": max((event["current_ltv"] for event in todays_events), default=None),
                "cumulative_liquidations": len(events),
                "cumulative_buyback_paid_usdc": cumulative_buyback,
                "cumulative_unwind_cost_usdc": cumulative_unwind,
                "cumulative_buffer_usdc": cumulative_buffer,
            }
        )

    return events, daily_rows


def summarize(events: list[dict[str, Any]], daily_rows: list[dict[str, Any]]) -> dict[str, Any]:
    failures = [event for event in events if event["buffer_usdc"] < -1e-9]
    worst_5d = 0
    if daily_rows:
        counts = [int(row["liquidated_today"]) for row in daily_rows]
        for idx in range(len(counts)):
            worst_5d = max(worst_5d, sum(counts[idx : idx + 5]))
    return {
        "liquidations": len(events),
        "failures": len(failures),
        "buyback_possible_all": len(failures) == 0,
        "total_buyback_paid_usdc": sum(event["buyback_price_usdc"] for event in events),
        "total_unwind_cost_usdc": sum(event["unwind_cost_usdc"] for event in events),
        "total_buffer_usdc": sum(event["buffer_usdc"] for event in events),
        "min_buffer_usdc": min((event["buffer_usdc"] for event in events), default=None),
        "min_coverage_ratio": min((event["coverage_ratio"] for event in events), default=None),
        "worst_single_day_buyback_count": max((int(row["liquidated_today"]) for row in daily_rows), default=0),
        "worst_5day_buyback_count": worst_5d,
    }


def summary_markdown(
    metadata: dict[str, Any],
    scenario: ScenarioConfig,
    primary_summary: dict[str, Any],
    stress_summary: dict[str, Any],
) -> str:
    source_line = f"- Source config: `{metadata['config_name']}`"
    return "\n".join(
        [
            "# Flagship Buyback Solvency",
            "",
            source_line,
            f"- Legacy hedge-lab root: `{metadata['legacy_root']}`",
            f"- Issued notes replayed: `{metadata['issued_windows']}` / `{metadata['candidate_windows']}` candidates",
            f"- Note notional: `${scenario.notional:.2f}`",
            f"- Buyback rule: `min(KI cap, current note liability - 10% notional)`",
            f"- KI cap: `${scenario.buyback_cap_usdc:.2f}` on `${scenario.notional:.2f}` notional",
            "- Current NAV source: note liability = daily pricer PV + accrued coupon liability",
            "- Available funds at liquidation: dedicated note balance sheet assets after immediate adverse wrapper unwind",
            "- Current production capital stack includes dedicated 12.5% junior first-loss capital per note",
            "",
            "## Primary",
            "",
            f"- Buybacks always payable: `{primary_summary['buyback_possible_all']}`",
            f"- Liquidations: `{primary_summary['liquidations']}`",
            f"- Failures: `{primary_summary['failures']}`",
            f"- Minimum buffer: `${0.0 if primary_summary['min_buffer_usdc'] is None else primary_summary['min_buffer_usdc']:.2f}`",
            f"- Minimum coverage ratio: `{0.0 if primary_summary['min_coverage_ratio'] is None else primary_summary['min_coverage_ratio']:.4f}x`",
            f"- Total buyback paid: `${primary_summary['total_buyback_paid_usdc']:.2f}`",
            f"- Worst single day: `{primary_summary['worst_single_day_buyback_count']}` buybacks",
            f"- Worst 5-day window: `{primary_summary['worst_5day_buyback_count']}` buybacks",
            "",
            "## Stress",
            "",
            f"- Stress liquidation test: `{scenario.forced_fraction:.0%}` of live notes on days with worst-asset 24h return <= `{scenario.stress_return_threshold:.0%}` or max 5d vol >= `{scenario.stress_vol_threshold:.0%}`",
            f"- Buybacks always payable: `{stress_summary['buyback_possible_all']}`",
            f"- Liquidations: `{stress_summary['liquidations']}`",
            f"- Failures: `{stress_summary['failures']}`",
            f"- Minimum buffer: `${0.0 if stress_summary['min_buffer_usdc'] is None else stress_summary['min_buffer_usdc']:.2f}`",
            f"- Minimum coverage ratio: `{0.0 if stress_summary['min_coverage_ratio'] is None else stress_summary['min_coverage_ratio']:.4f}x`",
            f"- Total buyback paid: `${stress_summary['total_buyback_paid_usdc']:.2f}`",
            f"- Worst single day: `{stress_summary['worst_single_day_buyback_count']}` buybacks",
            f"- Worst 5-day window: `{stress_summary['worst_5day_buyback_count']}` buybacks",
            "",
            "## Notes",
            "",
            "- The replay is coupled at the book level in the sense that liquidated notes are removed from all future days when portfolio concentration metrics are computed.",
            "- The current flagship hedge architecture is per-note and dedicated-balance-sheet, so one note's buyback does not perturb surviving notes' hedge state under the current production replay design.",
            "- Wrapper unwind stress remains assumption-driven because long stressed SPYX/QQQX/IWMX liquidity history is not directly observed in the research dataset.",
        ]
    ) + "\n"


def main() -> None:
    args = parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    if args.reuse_ledger and args.step_ledger_csv.exists():
        print(f"reusing flagship step ledger: {args.step_ledger_csv}", flush=True)
        ledger = load_step_ledger(args.step_ledger_csv)
        metadata = {
            "candidate_windows": None,
            "issued_windows": ledger["note_id"].nunique(),
            "asset_keys": [col.replace("_units", "") for col in ledger.columns if col.endswith("_units")],
            "config_name": "spy_qqq_iwm_factor_model_quarterly_recal_q65_cap500_daily",
            "legacy_root": str(LEGACY_HEDGE_LAB_ROOT),
            "source_summary_csv": str(DEFAULT_SOURCE_SUMMARY_CSV),
            "max_issued": args.max_issued,
        }
    else:
        ledger, metadata = generate_step_ledger(args)
        ledger.to_csv(args.step_ledger_csv, index=False)
        print(f"wrote flagship step ledger: {args.step_ledger_csv}", flush=True)

    cfg = build_cfg()
    notional = float(cfg["note"]["notional"])
    knock_in_barrier = float(cfg["note"]["knock_in_barrier"])
    hedge_legs = _hedge_legs(cfg, ["spy", "qqq", "iwm"])
    frame, asset_keys = load_multi_market_frame(cfg, LEGACY_HEDGE_LAB_ROOT)
    market_returns, market_vols = build_market_context(frame, asset_keys)

    scenario = ScenarioConfig(
        name="flagship_q65_cap500_daily",
        notional=notional,
        knock_in_barrier=knock_in_barrier,
        haircut_pct_of_notional=args.haircut_pct,
        initial_ltv=args.initial_ltv,
        liquidation_ltv=args.liquidation_ltv,
        stress_multiplier=args.stress_multiplier,
        stress_return_threshold=args.stress_return_threshold,
        stress_vol_threshold=args.stress_vol_threshold,
        forced_fraction=args.forced_fraction,
    )

    primary_events, primary_daily = primary_replay(ledger, scenario, cfg, hedge_legs, market_returns, market_vols)
    stress_events, stress_daily = stress_replay(ledger, scenario, cfg, hedge_legs, market_returns, market_vols)
    primary_summary = summarize(primary_events, primary_daily)
    stress_summary = summarize(stress_events, stress_daily)

    summary = {
        "source_step_csv": str(args.step_ledger_csv),
        "source_summary_csv": str(DEFAULT_SOURCE_SUMMARY_CSV),
        "metadata": metadata,
        "scenario": asdict(scenario),
        "primary": primary_summary,
        "stress": stress_summary,
    }

    summary_json = args.output_dir / "buyback_solvency_summary.json"
    summary_md = args.output_dir / "buyback_solvency_summary.md"
    primary_events_csv = args.output_dir / "buyback_solvency_primary_events.csv"
    primary_daily_csv = args.output_dir / "buyback_solvency_primary_daily.csv"
    stress_events_csv = args.output_dir / "buyback_solvency_stress_events.csv"
    stress_daily_csv = args.output_dir / "buyback_solvency_stress_daily.csv"

    summary_json.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    summary_md.write_text(summary_markdown(metadata, scenario, primary_summary, stress_summary), encoding="utf-8")
    pd.DataFrame(primary_events).to_csv(primary_events_csv, index=False)
    pd.DataFrame(primary_daily).to_csv(primary_daily_csv, index=False)
    pd.DataFrame(stress_events).to_csv(stress_events_csv, index=False)
    pd.DataFrame(stress_daily).to_csv(stress_daily_csv, index=False)

    print(summary_md.read_text(), flush=True)


if __name__ == "__main__":
    main()
