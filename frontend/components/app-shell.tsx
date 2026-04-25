"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { Droplets, Landmark, LineChart, Presentation, Settings2 } from "lucide-react";

import { Kingfisher } from "@/components/kingfisher";
import {
  EquityIcon,
  ILIcon,
  PortfolioIcon,
  SolIcon,
  VaultIcon,
} from "@/components/nav-icons";

import { useRuntimeConfig } from "@/lib/runtime-config";
import { cn, shortAddress } from "@/lib/format";
import { HALCYON_OPEN_RUNTIME_PANEL } from "@/lib/runtime-panel";
import { ClusterSwitchModal } from "@/components/cluster-switch-modal";
import { SettingsPanel } from "@/components/settings-panel";
import { WalletControl } from "@/components/wallet-control";

const NAV_ITEMS = [
  {
    href: "/demo",
    label: "Demo",
    description: "Receipt to collateral",
    icon: Presentation,
  },
  {
    href: "/flagship",
    label: "Equity Autocall",
    description: "SPY · QQQ · IWM coupon",
    icon: EquityIcon,
  },
  {
    href: "/il-protection",
    label: "IL Protection",
    description: "SOL/USDC LP cover",
    icon: ILIcon,
  },
  {
    href: "/sol-autocall",
    label: "SOL Autocall",
    description: "Principal-backed SOL note",
    icon: SolIcon,
  },
  {
    href: "/portfolio",
    label: "Portfolio",
    description: "Your open positions",
    icon: PortfolioIcon,
  },
  {
    href: "/lending-demo",
    label: "Lending Demo",
    description: "Receipt collateral desk",
    icon: Landmark,
  },
  {
    href: "/stress-tests",
    label: "Stress Tests",
    description: "Backtest explorer",
    icon: LineChart,
  },
  {
    href: "/faucet",
    label: "Faucet",
    description: "Devnet mockUSDC",
    icon: Droplets,
  },
  {
    href: "/vault",
    label: "Vault",
    description: "Underwriting capital",
    icon: VaultIcon,
  },
] as const;

function clusterTone(cluster: "localnet" | "devnet" | "mainnet") {
  if (cluster === "mainnet") return "text-success-700 border-success-700/30 bg-success-50";
  if (cluster === "devnet") return "text-halcyonBlue-700 border-halcyonBlue-300 bg-halcyonBlue-50";
  return "text-rust-700 border-rust-300 bg-rust-50";
}

function pageTitle(pathname: string) {
  const match = NAV_ITEMS.find((item) => pathname.startsWith(item.href));
  return match?.label ?? "Halcyon";
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const { cluster, current } = useRuntimeConfig();
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    const open = () => setSettingsOpen(true);
    window.addEventListener(HALCYON_OPEN_RUNTIME_PANEL, open);
    return () => window.removeEventListener(HALCYON_OPEN_RUNTIME_PANEL, open);
  }, []);

  return (
    <div className="min-h-screen bg-background">
      <div className="mx-auto flex min-h-screen max-w-[1600px] flex-col lg:flex-row">
        <aside className="border-b border-border bg-paper px-4 py-5 lg:sticky lg:top-0 lg:h-screen lg:w-[300px] lg:border-b-0 lg:border-r lg:px-5 lg:py-6">
          <Link
            href="/"
            className="flex items-start gap-3 rounded-md pb-5 border-b border-border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <Kingfisher size={32} color="var(--blue-600)" className="mt-1" />
            <div className="min-w-0">
              <div className="font-serif text-[22px] leading-none text-ink">Halcyon</div>
              <div className="mt-1 text-[10px] font-semibold uppercase tracking-[0.18em] text-n-400">
                Quant math · on-chain
              </div>
            </div>
          </Link>

          <div className="mt-5 hidden gap-3 lg:grid">
            {NAV_ITEMS.map((item) => {
              const active = pathname.startsWith(item.href);
              const Icon = item.icon;
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  className={cn(
                    "flex min-h-11 items-start gap-3 rounded-md border px-3 py-3 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                    active
                      ? "border-primary/30 bg-primary/10 text-foreground"
                      : "border-transparent bg-transparent text-muted-foreground hover:border-border hover:bg-secondary/70 hover:text-foreground",
                  )}
                >
                  <Icon className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                  <div className="min-w-0">
                    <div className="text-sm font-medium">{item.label}</div>
                    <div className="text-xs leading-5 text-muted-foreground">{item.description}</div>
                  </div>
                </Link>
              );
            })}
          </div>

          <div className="mt-5 flex gap-2 overflow-x-auto pb-1 lg:hidden">
            {NAV_ITEMS.map((item) => {
              const active = pathname.startsWith(item.href);
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  className={cn(
                    "flex min-h-11 shrink-0 items-center rounded-md border px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                    active
                      ? "border-primary/30 bg-primary/10 text-foreground"
                      : "border-border bg-card text-muted-foreground hover:bg-secondary/70 hover:text-foreground",
                  )}
                >
                  {item.label}
                </Link>
              );
            })}
          </div>

          <div className="mt-6 hidden rounded-md border border-border bg-card p-4 lg:block">
            <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
              Connected to
            </div>
            <div
              className={cn(
                "mt-3 inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium capitalize",
                clusterTone(cluster),
              )}
            >
              {cluster === "mainnet" ? "Solana mainnet" : cluster === "devnet" ? "Solana devnet" : "Local validator"}
            </div>
            <dl className="mt-4 space-y-3 text-sm">
              <div>
                <dt className="text-muted-foreground">RPC</dt>
                <dd className="mt-1 break-all font-mono text-[12px] leading-5 text-foreground">
                  {current.rpcUrl || "Not set"}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Protocol</dt>
                <dd className="mt-1 font-mono text-[12px] text-foreground">
                  {current.kernelProgramId ? shortAddress(current.kernelProgramId, 6) : "Not set"}
                </dd>
              </div>
            </dl>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="sticky top-0 z-30 border-b border-border bg-paper/90 backdrop-blur">
            <div className="flex flex-wrap items-center gap-3 px-4 py-4 sm:px-6">
              <div className="min-w-0 flex-1">
                <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                  Halcyon
                </div>
                <div className="truncate text-lg font-semibold text-foreground">{pageTitle(pathname)}</div>
              </div>

              <div
                className={cn(
                  "inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium",
                  clusterTone(cluster),
                )}
              >
                {cluster}
              </div>

              <button
                type="button"
                aria-label="Network settings"
                onClick={() => setSettingsOpen(true)}
                className="inline-flex min-h-10 min-w-10 items-center justify-center rounded-md border border-border bg-card px-2.5 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                <Settings2 className="h-4 w-4" aria-hidden="true" />
                <span className="ml-1.5 hidden sm:inline">Network</span>
              </button>

              <div className="min-h-10">
                <WalletControl />
              </div>
            </div>
          </header>

          <main className="flex-1 px-4 py-5 sm:px-6 sm:py-6">{children}</main>
        </div>
      </div>

      <SettingsPanel open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <ClusterSwitchModal />
    </div>
  );
}
