"use client";

import { useMemo, useState } from "react";
import {
  AlertTriangle,
  LoaderCircle,
  RefreshCw,
  Settings2,
} from "lucide-react";
import {
  createDefaultAddressSelector,
  createDefaultAuthorizationResultCache,
  createDefaultWalletNotFoundHandler,
  SolanaMobileWalletAdapter,
} from "@solana-mobile/wallet-adapter-mobile";
import type { Adapter } from "@solana/wallet-adapter-base";
import { BackpackWalletAdapter } from "@solana/wallet-adapter-backpack";
import { PhantomWalletAdapter } from "@solana/wallet-adapter-phantom";
import { ConnectionProvider, WalletProvider } from "@solana/wallet-adapter-react";
import { WalletModalProvider } from "@solana/wallet-adapter-react-ui";
import { SolflareWalletAdapter } from "@solana/wallet-adapter-solflare";
import { UnsafeBurnerWalletAdapter } from "@solana/wallet-adapter-unsafe-burner";

import { ClusterSwitchModal } from "@/components/cluster-switch-modal";
import { SettingsPanel } from "@/components/settings-panel";
import { RuntimeConfigProvider, useRuntimeConfig } from "@/lib/runtime-config";

import "@solana/wallet-adapter-react-ui/styles.css";

const MOBILE_APP_IDENTITY = { name: "Halcyon" } as const;

function mobileWalletChain(cluster: string) {
  if (cluster === "mainnet") return "solana:mainnet" as const;
  if (cluster === "devnet") return "solana:devnet" as const;
  return null;
}

function SolanaProviders({ children }: { children: React.ReactNode }) {
  const { cluster, current } = useRuntimeConfig();
  const wallets = useMemo(() => {
    const defaults: Adapter[] = [
      new PhantomWalletAdapter(),
      new SolflareWalletAdapter(),
      new BackpackWalletAdapter(),
    ];
    const mobileChain = mobileWalletChain(cluster);
    if (mobileChain) {
      defaults.push(
        new SolanaMobileWalletAdapter({
          addressSelector: createDefaultAddressSelector(),
          appIdentity: MOBILE_APP_IDENTITY,
          authorizationResultCache: createDefaultAuthorizationResultCache(),
          chain: mobileChain,
          onWalletNotFound: createDefaultWalletNotFoundHandler(),
        }),
      );
    }
    if (cluster === "localnet" && process.env.NEXT_PUBLIC_ENABLE_BURNER_WALLET === "1") {
      defaults.push(new UnsafeBurnerWalletAdapter());
    }
    return defaults;
  }, [cluster]);

  return (
    <ConnectionProvider endpoint={current.rpcUrl}>
      <WalletProvider wallets={wallets}>
        <WalletModalProvider>{children}</WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  );
}

function RuntimeGate({ children }: { children: React.ReactNode }) {
  const { current, genesisCheck, retryGenesisCheck } = useRuntimeConfig();
  const [settingsOpen, setSettingsOpen] = useState(false);

  if (genesisCheck.status === "pending" || genesisCheck.status === "error") {
    const blocking = genesisCheck.status === "error";

    return (
      <>
        <div className="min-h-screen bg-background">
          <div className="mx-auto flex min-h-screen max-w-[960px] items-center px-4 py-10 sm:px-6">
            <section
              aria-busy={!blocking}
              className="w-full rounded-md border border-border bg-card p-6 shadow-sm sm:p-8"
              data-testid={blocking ? "genesis-check-blocked" : "genesis-check-pending"}
              role={blocking ? "alert" : "status"}
            >
              <div className="flex items-start gap-3">
                <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-md border border-border bg-background">
                  {blocking ? (
                    <AlertTriangle className="h-5 w-5 text-error-700" aria-hidden="true" />
                  ) : (
                    <LoaderCircle
                      className="h-5 w-5 animate-spin motion-reduce:animate-none"
                      aria-hidden="true"
                    />
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <p className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                    Runtime Guard
                  </p>
                  <h1 className="mt-1 text-2xl font-semibold text-foreground">
                    {blocking ? "Cluster verification failed" : "Verifying RPC cluster"}
                  </h1>
                  <p className="mt-3 text-sm leading-6 text-muted-foreground">
                    {blocking
                      ? genesisCheck.reason
                      : `Checking that the selected RPC matches the pinned ${current.label} genesis hash before the wallet can connect.`}
                  </p>
                  <p className="mt-3 text-sm leading-6 text-muted-foreground">
                    {blocking
                      ? "Wallet connection stays disabled until the selected endpoint passes the genesis check. Switch to a compliant cluster or retry after the RPC is fixed."
                      : "The wallet provider stays offline until this check completes."}
                  </p>
                  <div className="mt-5 flex flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={() => setSettingsOpen(true)}
                      className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                    >
                      <Settings2 className="h-4 w-4" aria-hidden="true" />
                      Runtime Config
                    </button>
                    {blocking ? (
                      <button
                        type="button"
                        onClick={retryGenesisCheck}
                        className="inline-flex min-h-10 items-center gap-2 rounded-md bg-primary px-3 text-sm font-semibold text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                      >
                        <RefreshCw className="h-4 w-4" aria-hidden="true" />
                        Try Again
                      </button>
                    ) : null}
                  </div>
                </div>
              </div>
            </section>
          </div>
        </div>
        <SettingsPanel open={settingsOpen} onClose={() => setSettingsOpen(false)} />
        <ClusterSwitchModal />
      </>
    );
  }

  return <SolanaProviders>{children}</SolanaProviders>;
}

export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <RuntimeConfigProvider>
      <RuntimeGate>{children}</RuntimeGate>
    </RuntimeConfigProvider>
  );
}
