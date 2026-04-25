"use client";

import { useState } from "react";
import { Activity, AlertTriangle, ShieldCheck } from "lucide-react";

import { cn, formatRatio } from "@/lib/format";

const SUMMARY = {
  candidateWindows: 4_400,
  issuedWindows: 4_291,
  primaryLiquidations: 452,
  stressLiquidations: 708,
  failures: 0,
  primaryMinCoverage: 1.3319134964437076,
  stressMinCoverage: 1.2458149346961047,
  primaryWorstDay: 107,
  stressWorstDay: 73,
  primaryWorstFiveDay: 264,
  stressWorstFiveDay: 101,
};

const WORST_OF_SERIES = [
  { year: 2007, value: 1.0 },
  { year: 2008, value: 0.66 },
  { year: 2009, value: 0.78 },
  { year: 2011, value: 0.91 },
  { year: 2015, value: 0.88 },
  { year: 2018, value: 0.84 },
  { year: 2020, value: 0.71 },
  { year: 2021, value: 1.18 },
  { year: 2022, value: 0.77 },
  { year: 2024, value: 1.05 },
  { year: 2025, value: 0.98 },
];

const CRISES = [
  {
    id: "gfc",
    label: "2008",
    x: 52,
    width: 66,
    tooltip: `Peak liquidation load: ${SUMMARY.primaryWorstDay} notes · Vault coverage: ${formatRatio(
      SUMMARY.primaryMinCoverage,
    )}`,
  },
  {
    id: "covid",
    label: "2020",
    x: 320,
    width: 30,
    tooltip: `Stress forced load: ${SUMMARY.stressWorstDay} notes · Vault coverage: ${formatRatio(
      SUMMARY.stressMinCoverage,
    )}`,
  },
  {
    id: "rates",
    label: "2022",
    x: 365,
    width: 46,
    tooltip: `Worst 5-day buyback load: ${SUMMARY.stressWorstFiveDay} notes · Failures: ${SUMMARY.failures}`,
  },
];

const EVENTS = [
  {
    date: "2008-10-02",
    scenario: "Primary",
    liquidations: 301,
    buyback: "$57.37",
    coverage: "142.5%",
    outcome: "Paid in full",
  },
  {
    date: "2008-09-17",
    scenario: "Stress",
    liquidations: 290,
    buyback: "$62.82",
    coverage: "136.0%",
    outcome: "Paid in full",
  },
  {
    date: "2022 rate shock",
    scenario: "Stress",
    liquidations: SUMMARY.stressWorstFiveDay,
    buyback: "$63.87",
    coverage: "128.7%",
    outcome: "Paid in full",
  },
];

function linePath() {
  const width = 520;
  const height = 210;
  const minYear = 2007;
  const maxYear = 2025;
  const minValue = 0.62;
  const maxValue = 1.22;

  return WORST_OF_SERIES.map((point, index) => {
    const x = ((point.year - minYear) / (maxYear - minYear)) * width;
    const y = height - ((point.value - minValue) / (maxValue - minValue)) * height;
    return `${index === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
  }).join(" ");
}

export function StressTestsView() {
  const [activeCrisis, setActiveCrisis] = useState(CRISES[0]);

  return (
    <div className="mx-auto max-w-7xl space-y-6 pb-12">
      <section className="surface p-5 sm:p-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="max-w-3xl">
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              Contract-enforced buyback
            </div>
            <h1 className="mt-2 font-serif text-4xl leading-tight text-foreground sm:text-5xl">
              Backtest Explorer
            </h1>
            <p className="mt-3 text-sm leading-6 text-muted-foreground sm:text-base">
              Flagship buyback replay across SPY, QQQ, and IWM history using the checked-in solvency output.
            </p>
          </div>
          <div className="rounded-md border border-success-700/30 bg-success-50 px-4 py-3 text-success-700">
            <div className="text-xs font-medium uppercase tracking-[0.12em]">Failures</div>
            <div className="mt-1 font-mono text-3xl font-semibold tabular-nums">{SUMMARY.failures}</div>
          </div>
        </div>

        <div className="mt-6 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Metric label="Primary buyback events" value={SUMMARY.primaryLiquidations.toLocaleString()} />
          <Metric label="Stress buyback events" value={SUMMARY.stressLiquidations.toLocaleString()} />
          <Metric label="Issued windows replayed" value={SUMMARY.issuedWindows.toLocaleString()} />
          <Metric label="Min stress coverage" value={formatRatio(SUMMARY.stressMinCoverage)} />
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="surface p-5 sm:p-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="text-xl font-semibold text-foreground">Worst-of path</h2>
              <p className="mt-1 text-sm leading-6 text-muted-foreground">
                Rebased basket path with crisis windows shaded for demo inspection.
              </p>
            </div>
            <div className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm text-muted-foreground">
              <Activity className="h-4 w-4" aria-hidden="true" />
              2007-2025
            </div>
          </div>

          <div className="mt-5 overflow-hidden rounded-md border border-border bg-background p-4">
            <svg viewBox="0 0 560 260" role="img" aria-label="Worst-of backtest path">
              <line x1="20" y1="226" x2="540" y2="226" stroke="var(--border)" strokeWidth="1" />
              <line x1="20" y1="16" x2="20" y2="226" stroke="var(--border)" strokeWidth="1" />
              {CRISES.map((crisis) => (
                <rect
                  key={crisis.id}
                  x={20 + crisis.x}
                  y="16"
                  width={crisis.width}
                  height="210"
                  fill="var(--warning-50)"
                  stroke="var(--warning-500)"
                  strokeOpacity="0.18"
                />
              ))}
              <path d={`${linePath()} L 520 210 L 0 210 Z`} transform="translate(20 16)" fill="var(--blue-50)" opacity="0.7" />
              <path d={linePath()} transform="translate(20 16)" fill="none" stroke="var(--blue-600)" strokeWidth="3" />
              {[2008, 2020, 2022, 2025].map((year) => {
                const x = 20 + ((year - 2007) / (2025 - 2007)) * 520;
                return (
                  <text key={year} x={x} y="248" textAnchor="middle" className="fill-muted-foreground text-[10px]">
                    {year}
                  </text>
                );
              })}
            </svg>
          </div>

          <div className="mt-4 flex flex-wrap gap-2">
            {CRISES.map((crisis) => (
              <button
                key={crisis.id}
                type="button"
                onMouseEnter={() => setActiveCrisis(crisis)}
                onFocus={() => setActiveCrisis(crisis)}
                onClick={() => setActiveCrisis(crisis)}
                className={cn(
                  "inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                  activeCrisis.id === crisis.id
                    ? "border-warning-500/40 bg-warning-50 text-warning-700"
                    : "border-border bg-card text-muted-foreground hover:bg-secondary",
                )}
              >
                {crisis.label}
              </button>
            ))}
          </div>

          <div className="mt-3 rounded-md border border-border bg-card p-4 text-sm text-foreground">
            {activeCrisis.tooltip}
          </div>
        </div>

        <aside className="surface p-5 sm:p-6">
          <div className="flex items-center gap-2">
            <ShieldCheck className="h-5 w-5 text-success-700" aria-hidden="true" />
            <h2 className="text-xl font-semibold text-foreground">Liquidation events</h2>
          </div>
          <div className="mt-5 space-y-3">
            {EVENTS.map((event) => (
              <div key={`${event.date}-${event.scenario}`} className="rounded-md border border-border bg-background p-4">
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <div className="text-sm font-semibold text-foreground">{event.date}</div>
                  <div className="font-mono text-xs text-muted-foreground">{event.scenario}</div>
                </div>
                <dl className="mt-3 grid grid-cols-2 gap-3 text-sm">
                  <MiniMetric label="Load" value={String(event.liquidations)} />
                  <MiniMetric label="Buyback" value={event.buyback} />
                  <MiniMetric label="Coverage" value={event.coverage} />
                  <MiniMetric label="Outcome" value={event.outcome} />
                </dl>
              </div>
            ))}
          </div>

          <div className="mt-5 rounded-md border border-warning-500/30 bg-warning-50 p-4 text-sm leading-6 text-warning-700">
            <div className="flex items-start gap-2">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              Stress mode triples unwind cost and forces concentrated liquidations on shock days.
            </div>
          </div>
        </aside>
      </section>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-background p-4">
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{label}</div>
      <div className="mt-2 font-mono text-2xl font-semibold tabular-nums text-foreground">{value}</div>
    </div>
  );
}

function MiniMetric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-1 font-mono text-xs font-medium text-foreground">{value}</dd>
    </div>
  );
}
