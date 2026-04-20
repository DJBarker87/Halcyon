"use client";

import { useMemo } from "react";
import type { Adapter } from "@solana/wallet-adapter-base";
import { ConnectionProvider, WalletProvider } from "@solana/wallet-adapter-react";
import {
  PhantomWalletAdapter,
  SolflareWalletAdapter,
  UnsafeBurnerWalletAdapter,
} from "@solana/wallet-adapter-wallets";
import { WalletModalProvider } from "@solana/wallet-adapter-react-ui";

import { RuntimeConfigProvider, useRuntimeConfig } from "@/lib/runtime-config";

import "@solana/wallet-adapter-react-ui/styles.css";

function SolanaProviders({ children }: { children: React.ReactNode }) {
  const { cluster, current } = useRuntimeConfig();
  const wallets = useMemo(() => {
    const defaults: Adapter[] = [new PhantomWalletAdapter(), new SolflareWalletAdapter()];
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

export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <RuntimeConfigProvider>
      <SolanaProviders>{children}</SolanaProviders>
    </RuntimeConfigProvider>
  );
}
