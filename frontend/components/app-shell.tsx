"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import {
  AlertTriangle,
  BriefcaseBusiness,
  ChartColumn,
  Cpu,
  Database,
  Layers3,
  Settings2,
  ShieldCheck,
  Wallet,
} from "lucide-react";
import { WalletMultiButton } from "@solana/wallet-adapter-react-ui";

import { useRuntimeConfig } from "@/lib/runtime-config";
import { cn, shortAddress } from "@/lib/format";
import { HALCYON_OPEN_RUNTIME_PANEL } from "@/lib/runtime-panel";
import { SettingsPanel } from "@/components/settings-panel";

const NAV_ITEMS = [
  {
    href: "/flagship",
    label: "Flagship",
    description: "Worst-of equity autocall",
    icon: ChartColumn,
  },
  {
    href: "/il-protection",
    label: "IL Protection",
    description: "Synthetic SOL/USDC cover",
    icon: ShieldCheck,
  },
  {
    href: "/sol-autocall",
    label: "SOL Autocall",
    description: "Principal-backed note",
    icon: Cpu,
  },
  {
    href: "/portfolio",
    label: "Portfolio",
    description: "Live policies by wallet",
    icon: BriefcaseBusiness,
  },
  {
    href: "/vault",
    label: "Vault",
    description: "Kernel capital state",
    icon: Database,
  },
] as const;

function clusterTone(cluster: "localnet" | "devnet" | "mainnet") {
  if (cluster === "mainnet") return "text-emerald-300 border-emerald-400/30 bg-emerald-400/10";
  if (cluster === "devnet") return "text-cyan-300 border-cyan-400/30 bg-cyan-400/10";
  return "text-amber-300 border-amber-400/30 bg-amber-400/10";
}

function pageTitle(pathname: string) {
  const match = NAV_ITEMS.find((item) => pathname.startsWith(item.href));
  return match?.label ?? "Halcyon";
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const { cluster, current, currentHasOverrides } = useRuntimeConfig();
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    const open = () => setSettingsOpen(true);
    window.addEventListener(HALCYON_OPEN_RUNTIME_PANEL, open);
    return () => window.removeEventListener(HALCYON_OPEN_RUNTIME_PANEL, open);
  }, []);

  return (
    <div className="min-h-screen bg-background">
      <div className="mx-auto flex min-h-screen max-w-[1600px] flex-col lg:flex-row">
        <aside className="border-b border-border bg-card/60 px-4 py-5 backdrop-blur lg:sticky lg:top-0 lg:h-screen lg:w-[300px] lg:border-b-0 lg:border-r lg:px-5 lg:py-6">
          <Link
            href="/flagship"
            className="flex items-start gap-3 rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <div className="flex h-11 w-11 items-center justify-center rounded-md border border-border bg-background">
              <Layers3 className="h-5 w-5" aria-hidden="true" />
            </div>
            <div className="min-w-0">
              <div className="text-sm font-medium uppercase tracking-[0.12em] text-muted-foreground">
                Halcyon
              </div>
              <div className="text-xl font-semibold text-foreground">Layer 5 Console</div>
              <p className="mt-1 text-sm leading-6 text-muted-foreground">
                The new wired frontend. The WASM demo stays in `app/`.
              </p>
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

          <div className="mt-6 hidden rounded-md border border-border bg-background/70 p-4 lg:block">
            <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
              Runtime
            </div>
            <div
              className={cn(
                "mt-3 inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium",
                clusterTone(cluster),
              )}
            >
              {cluster}
            </div>
            <dl className="mt-4 space-y-3 text-sm">
              <div>
                <dt className="text-muted-foreground">RPC</dt>
                <dd className="mt-1 break-all font-mono text-[12px] leading-5 text-foreground">
                  {current.rpcUrl || "Not set"}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Kernel</dt>
                <dd className="mt-1 font-mono text-[12px] text-foreground">
                  {current.kernelProgramId ? shortAddress(current.kernelProgramId, 6) : "Not set"}
                </dd>
              </div>
            </dl>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="sticky top-0 z-30 border-b border-border bg-background/85 backdrop-blur">
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
                onClick={() => setSettingsOpen(true)}
                className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                <Settings2 className="h-4 w-4" aria-hidden="true" />
                Runtime Config
              </button>

              <div className="min-h-10">
                <WalletMultiButton />
              </div>
            </div>

            {currentHasOverrides && (
              <div className="border-t border-amber-400/20 bg-amber-400/10 px-4 py-3 text-sm text-foreground sm:px-6">
                <div className="flex items-start gap-2">
                  <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-300" aria-hidden="true" />
                  <p>
                    Browser-local runtime overrides are active. Review the selected cluster, RPC, program IDs,
                    and oracle accounts before signing.
                  </p>
                </div>
              </div>
            )}
          </header>

          <main className="flex-1 px-4 py-5 sm:px-6 sm:py-6">{children}</main>
        </div>
      </div>

      <SettingsPanel open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
