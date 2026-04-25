#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import math
import os
import subprocess
import sys
import time
import urllib.parse
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Iterable

from calibration import (
    CROSSCHECK_DATE,
    CROSSCHECK_EXPECTED_SIGMA_S6,
    CROSSCHECK_MAX_SIGMA_DRIFT_PCT,
    ELL_SPY,
    EWMA_DECAY,
    EWMA_LOOKBACK_DAYS,
    EWMA_MIN_PERIODS,
    PYTH_BENCHMARKS_BASE_URL,
    PYTH_HISTORY_START_DATE,
    RESIDUAL_COV_SPY_DAILY,
    SPY_PYTH_FEED_ID,
    SPY_PYTH_SYMBOL,
    TRADING_DAYS_PER_YEAR,
)

USER_AGENT = "Mozilla/5.0 (compatible; Halcyon Flagship Sigma Keeper)"
PYTH_MAX_HISTORY_RANGE_DAYS = 364
PYTH_CACHE_FRESH_GRACE_DAYS = 3
PYTH_HTTP_MAX_ATTEMPTS = 6


@dataclass(frozen=True)
class PricePoint:
    timestamp: datetime
    close: float


@dataclass(frozen=True)
class SigmaResult:
    as_of: str
    annual_vol: float
    sigma_common: float
    sigma_s6: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compute flagship sigma_common from SPY history and optionally submit it."
    )
    parser.add_argument(
        "--spy-history-source",
        default=os.getenv("FLAGSHIP_SIGMA_SPY_HISTORY_SOURCE", f"pyth:{SPY_PYTH_FEED_ID}"),
        help="SPY history source. Either a CSV path or pyth:FEED_ID / pyth:SYMBOL.",
    )
    parser.add_argument(
        "--crosscheck-history-source",
        default=os.getenv("FLAGSHIP_SIGMA_CROSSCHECK_SOURCE", f"pyth:{SPY_PYTH_FEED_ID}"),
        help="SPY history source used for the fixed-date cross-check.",
    )
    parser.add_argument(
        "--cache-path",
        default=os.getenv("FLAGSHIP_SIGMA_CACHE_PATH", "/var/lib/halcyon/spy_1d.csv"),
        help="Optional cache path for fetched daily history.",
    )
    parser.add_argument(
        "--pyth-benchmarks-base-url",
        default=os.getenv("FLAGSHIP_SIGMA_PYTH_BENCHMARKS_BASE_URL", PYTH_BENCHMARKS_BASE_URL),
        help="Base URL for the Pyth Benchmarks API.",
    )
    parser.add_argument(
        "--date",
        default=None,
        help="Optional YYYY-MM-DD date to price. Defaults to the latest history row.",
    )
    parser.add_argument(
        "--rpc",
        default=os.getenv("HELIUS_DEVNET_RPC"),
        help="RPC URL passed through to the Halcyon CLI in submit mode.",
    )
    parser.add_argument(
        "--halcyon-bin",
        default=os.getenv("HALCYON_BIN", "/opt/halcyon/bin/halcyon"),
        help="Path to the Halcyon CLI binary.",
    )
    parser.add_argument(
        "--keypair",
        default=os.getenv("FLAGSHIP_SIGMA_KEYPAIR"),
        help="Keypair passed through to the Halcyon CLI in submit mode.",
    )
    parser.add_argument(
        "--publish-ts",
        type=int,
        default=None,
        help="Optional publish timestamp passed through to the Halcyon CLI.",
    )
    parser.add_argument(
        "--publish-slot",
        type=int,
        default=None,
        help="Optional publish slot passed through to the Halcyon CLI.",
    )
    parser.add_argument(
        "--skip-crosscheck",
        action="store_true",
        help="Skip the fixed-date cross-check. Not recommended.",
    )
    parser.add_argument(
        "--submit",
        action="store_true",
        help="Submit the computed sigma through `halcyon keepers write-sigma-value`.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    cache_path = Path(args.cache_path) if args.cache_path else None
    if not args.skip_crosscheck:
        crosscheck_cache_path = (
            cache_path if args.crosscheck_history_source == args.spy_history_source else None
        )
        run_crosscheck(args.crosscheck_history_source, args.pyth_benchmarks_base_url, crosscheck_cache_path)

    history = load_history(
        args.spy_history_source,
        cache_path,
        args.pyth_benchmarks_base_url,
        args.date,
    )
    result = compute_sigma_result(history, args.date)
    print_result(result, args.spy_history_source)

    if args.submit:
        submit_sigma(args, result.sigma_s6)

    return 0


def run_crosscheck(source: str, benchmarks_base_url: str, cache_path: Path | None) -> None:
    history = load_history(
        source,
        cache_path=cache_path,
        benchmarks_base_url=benchmarks_base_url,
        end_date=CROSSCHECK_DATE,
    )
    result = compute_sigma_result(history, CROSSCHECK_DATE)
    diff_pct = abs(result.sigma_s6 - CROSSCHECK_EXPECTED_SIGMA_S6) * 100.0 / CROSSCHECK_EXPECTED_SIGMA_S6
    if result.sigma_s6 == CROSSCHECK_EXPECTED_SIGMA_S6:
        print(
            json.dumps(
                {
                    "crosscheck_date": CROSSCHECK_DATE,
                    "crosscheck_sigma_s6": result.sigma_s6,
                    "crosscheck_diff_pct": 0.0,
                    "status": "exact-match",
                },
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return
    if diff_pct < CROSSCHECK_MAX_SIGMA_DRIFT_PCT:
        print(
            json.dumps(
                {
                    "crosscheck_date": CROSSCHECK_DATE,
                    "crosscheck_sigma_s6": result.sigma_s6,
                    "crosscheck_expected_sigma_s6": CROSSCHECK_EXPECTED_SIGMA_S6,
                    "crosscheck_diff_pct": diff_pct,
                    "status": "within-tolerance",
                },
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return
    raise SystemExit(
        "cross-check failed: "
        f"{CROSSCHECK_DATE} expected sigma_s6={CROSSCHECK_EXPECTED_SIGMA_S6}, "
        f"got {result.sigma_s6} ({diff_pct:.3f}% drift, "
        f"limit {CROSSCHECK_MAX_SIGMA_DRIFT_PCT:.3f}%)"
    )


def load_history(
    source: str,
    cache_path: Path | None,
    benchmarks_base_url: str,
    end_date: str | None,
) -> list[PricePoint]:
    if source.lower().startswith("pyth:"):
        identifier = source.split(":", 1)[1] or SPY_PYTH_FEED_ID
        return fetch_pyth_history(identifier, cache_path, benchmarks_base_url, end_date)
    return read_history_csv(Path(source))


def fetch_pyth_history(
    identifier: str,
    cache_path: Path | None,
    benchmarks_base_url: str,
    end_date: str | None,
) -> list[PricePoint]:
    if cache_path is not None:
        try:
            cache_path.parent.mkdir(parents=True, exist_ok=True)
        except PermissionError:
            cache_path = None

    symbol = resolve_pyth_symbol(identifier, benchmarks_base_url)
    start = date.fromisoformat(PYTH_HISTORY_START_DATE)
    final_date = date.fromisoformat(end_date) if end_date else datetime.now(timezone.utc).date()
    if final_date < start:
        raise ValueError(f"end date {final_date.isoformat()} is earlier than start {start.isoformat()}")

    rows: dict[str, PricePoint] = {}
    chunk_start = start
    if cache_path is not None and cache_path.exists():
        try:
            cached_rows = [
                point
                for point in read_history_csv(cache_path)
                if start <= point.timestamp.date() <= final_date
            ]
        except (OSError, ValueError) as exc:
            print(
                json.dumps(
                    {
                        "cache_path": str(cache_path),
                        "cache_status": "ignored",
                        "reason": str(exc),
                    },
                    sort_keys=True,
                ),
                file=sys.stderr,
            )
        else:
            for point in cached_rows:
                rows[point.timestamp.date().isoformat()] = point
            if rows:
                cached_last = max(date.fromisoformat(day) for day in rows)
                if cached_last >= final_date:
                    return sorted(rows.values(), key=lambda row: row.timestamp)
                if end_date is None and (final_date - cached_last).days <= PYTH_CACHE_FRESH_GRACE_DAYS:
                    return sorted(rows.values(), key=lambda row: row.timestamp)
                chunk_start = cached_last + timedelta(days=1)

    while chunk_start <= final_date:
        chunk_end = min(chunk_start + timedelta(days=PYTH_MAX_HISTORY_RANGE_DAYS), final_date)
        chunk_rows = fetch_pyth_history_chunk(symbol, chunk_start, chunk_end, benchmarks_base_url)
        for point in chunk_rows:
            rows[point.timestamp.date().isoformat()] = point
        chunk_start = chunk_end + timedelta(days=1)

    ordered = sorted(rows.values(), key=lambda row: row.timestamp)
    if not ordered:
        raise ValueError(f"no usable price rows returned by Pyth Benchmarks for {symbol}")

    if cache_path is not None:
        with cache_path.open("w", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["date", "open", "high", "low", "close", "volume"])
            for row in ordered:
                writer.writerow([row.timestamp.isoformat(sep=" "), "", "", "", row.close, ""])
    return ordered


def resolve_pyth_symbol(identifier: str, benchmarks_base_url: str) -> str:
    lowered = identifier.lower()
    if "/" in identifier and not lowered.startswith("0x"):
        return identifier
    feed_id = identifier if lowered.startswith("0x") else f"0x{identifier}"
    request = urllib.request.Request(
        f"{benchmarks_base_url.rstrip('/')}/v1/price_feeds/{feed_id}",
        headers={"User-Agent": USER_AGENT},
    )
    payload = fetch_json_with_retry(request)
    return payload.get("attributes", {}).get("symbol", SPY_PYTH_SYMBOL)


def fetch_pyth_history_chunk(
    symbol: str,
    start_date: date,
    end_date: date,
    benchmarks_base_url: str,
) -> list[PricePoint]:
    if end_date < start_date:
        return []

    from_ts = int(datetime(start_date.year, start_date.month, start_date.day, tzinfo=timezone.utc).timestamp())
    to_ts = int(datetime(end_date.year, end_date.month, end_date.day, tzinfo=timezone.utc).timestamp())
    query = urllib.parse.urlencode(
        {
            "symbol": symbol,
            "resolution": "1D",
            "from": from_ts,
            "to": to_ts,
        },
        safe="/",
    )
    request = urllib.request.Request(
        f"{benchmarks_base_url.rstrip('/')}/v1/shims/tradingview/history?{query}",
        headers={"User-Agent": USER_AGENT},
    )
    payload = fetch_json_with_retry(request)
    if payload.get("s") != "ok":
        raise ValueError(f"Pyth Benchmarks history error for {symbol}: {payload}")

    rows: list[PricePoint] = []
    for ts, close in zip(payload.get("t", []), payload.get("c", [])):
        if close is None:
            continue
        point = PricePoint(
            # Benchmarks daily bars use UTC-midnight timestamps that act as day labels.
            timestamp=datetime.fromtimestamp(int(ts), tz=timezone.utc).replace(tzinfo=None),
            close=float(close),
        )
        if point.close > 0.0 and math.isfinite(point.close):
            rows.append(point)
    return rows


def fetch_json_with_retry(request: urllib.request.Request) -> object:
    for attempt in range(PYTH_HTTP_MAX_ATTEMPTS):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return json.load(response)
        except urllib.error.HTTPError as exc:
            if exc.code != 429 or attempt + 1 >= PYTH_HTTP_MAX_ATTEMPTS:
                raise
            retry_after = exc.headers.get("retry-after")
            try:
                sleep_secs = float(retry_after) if retry_after else 0.0
            except ValueError:
                sleep_secs = 0.0
            if sleep_secs <= 0.0:
                sleep_secs = min(2.0 ** attempt, 30.0)
            time.sleep(sleep_secs)
    raise RuntimeError("unreachable")


def read_history_csv(path: Path) -> list[PricePoint]:
    rows: list[PricePoint] = []
    with path.open(newline="") as handle:
        for raw in csv.DictReader(handle):
            close = float(raw["close"])
            if close <= 0.0 or not math.isfinite(close):
                continue
            rows.append(
                PricePoint(
                    timestamp=datetime.fromisoformat(raw["date"].strip()),
                    close=close,
                )
            )
    if not rows:
        raise ValueError(f"no usable price rows in {path}")
    rows.sort(key=lambda row: row.timestamp)
    return rows


def compute_sigma_result(history: list[PricePoint], date_filter: str | None) -> SigmaResult:
    closes = [row.close for row in history]
    annual_vols = compute_ewma_annual_vol(closes)

    index = resolve_index(history, date_filter)
    annual_vol = annual_vols[index]
    factor_annual_var = max(
        annual_vol * annual_vol - RESIDUAL_COV_SPY_DAILY * TRADING_DAYS_PER_YEAR,
        1e-12,
    )
    sigma_common = math.sqrt(factor_annual_var) / max(ELL_SPY, 1e-12)
    sigma_s6 = int(round(sigma_common * 1_000_000.0))
    as_of = history[index].timestamp.date().isoformat()
    return SigmaResult(
        as_of=as_of,
        annual_vol=annual_vol,
        sigma_common=sigma_common,
        sigma_s6=sigma_s6,
    )


def resolve_index(history: list[PricePoint], date_filter: str | None) -> int:
    if date_filter is None:
        return len(history) - 1
    for idx, row in enumerate(history):
        if row.timestamp.date().isoformat() == date_filter:
            return idx
    raise ValueError(f"date {date_filter} not present in history")


def compute_ewma_annual_vol(closes: Iterable[float]) -> list[float]:
    prices = list(closes)
    if not prices:
        raise ValueError("empty close series")

    log_returns = [0.0]
    for prev, nxt in zip(prices, prices[1:]):
        if prev <= 0.0 or nxt <= 0.0:
            raise ValueError("close prices must be positive")
        log_returns.append(math.log(nxt / prev))

    variance: list[float] = []
    prev_var = 0.0
    for value in log_returns:
        prev_var = EWMA_DECAY * prev_var + (1.0 - EWMA_DECAY) * (float(value) ** 2)
        variance.append(prev_var)

    annualised = [math.sqrt(value * TRADING_DAYS_PER_YEAR) for value in variance]
    rolled: list[float | None] = [None] * len(annualised)
    for idx in range(len(annualised)):
        start = max(0, idx - EWMA_LOOKBACK_DAYS + 1)
        window = annualised[start : idx + 1]
        if len(window) >= EWMA_MIN_PERIODS:
            rolled[idx] = sum(window) / len(window)

    mean_value = mean_non_none(rolled)
    backfilled = backfill(rolled)
    return [max(value if value is not None else mean_value, 1e-4) for value in backfilled]


def mean_non_none(values: Iterable[float | None]) -> float:
    filtered = [value for value in values if value is not None]
    if not filtered:
        raise ValueError("rolling EWMA series contains no populated points")
    return sum(filtered) / len(filtered)


def backfill(values: list[float | None]) -> list[float | None]:
    out = list(values)
    next_value: float | None = None
    for idx in range(len(out) - 1, -1, -1):
        if out[idx] is None:
            out[idx] = next_value
        else:
            next_value = out[idx]
    return out


def print_result(result: SigmaResult, source: str) -> None:
    payload = {
        "source": source,
        "as_of": result.as_of,
        "annual_vol": result.annual_vol,
        "sigma_common": result.sigma_common,
        "sigma_s6": result.sigma_s6,
    }
    print(json.dumps(payload, indent=2, sort_keys=True))


def submit_sigma(args: argparse.Namespace, sigma_s6: int) -> None:
    if not args.rpc:
        raise SystemExit("--rpc or HELIUS_DEVNET_RPC is required for --submit")
    if not args.keypair:
        raise SystemExit("--keypair or FLAGSHIP_SIGMA_KEYPAIR is required for --submit")

    command = [
        args.halcyon_bin,
        "--rpc",
        args.rpc,
        "--keypair",
        args.keypair,
        "keepers",
        "write-sigma-value",
        "--sigma-annualised-s6",
        str(sigma_s6),
    ]
    if args.publish_ts is not None:
        command.extend(["--publish-ts", str(args.publish_ts)])
    if args.publish_slot is not None:
        command.extend(["--publish-slot", str(args.publish_slot)])

    subprocess.run(command, check=True)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
