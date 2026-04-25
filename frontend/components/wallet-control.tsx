"use client";

import { useEffect, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Copy,
  LoaderCircle,
  LogOut,
  RefreshCw,
  Wallet,
} from "lucide-react";
import { WalletReadyState } from "@solana/wallet-adapter-base";
import type { Wallet as AdapterWallet } from "@solana/wallet-adapter-react";
import { useWallet } from "@solana/wallet-adapter-react";
import { useWalletModal } from "@solana/wallet-adapter-react-ui";

import { cn, shortAddress } from "@/lib/format";

function canOpenWallet(wallet: AdapterWallet | null) {
  return wallet?.readyState === WalletReadyState.Installed || wallet?.readyState === WalletReadyState.Loadable;
}

function selectedWalletIssue(wallet: AdapterWallet | null) {
  if (!wallet) return "Choose a wallet from the list to continue.";

  if (wallet.readyState === WalletReadyState.NotDetected) {
    return `${wallet.adapter.name} is not detected in this browser. Install or unlock the extension, then choose it again.`;
  }

  if (wallet.readyState === WalletReadyState.Unsupported) {
    return `${wallet.adapter.name} is not supported in this browser. Choose a different wallet.`;
  }

  return `${wallet.adapter.name} is not ready. Unlock the extension and try again.`;
}

function walletConnectErrorMessage(cause: unknown, wallet: AdapterWallet | null) {
  const name = cause instanceof Error ? cause.name : "";
  const message = cause instanceof Error ? cause.message : "";
  const normalized = `${name} ${message}`.toLowerCase();

  if (normalized.includes("notselected")) {
    return "Choose a wallet from the list to continue.";
  }

  if (normalized.includes("notready")) {
    return selectedWalletIssue(wallet);
  }

  if (normalized.includes("rejected") || normalized.includes("cancel")) {
    return "The wallet request was cancelled.";
  }

  if (normalized.includes("blocked")) {
    return "The browser blocked the wallet window. Allow popups for this site, then try again.";
  }

  if (normalized.includes("timeout")) {
    return "The wallet did not respond in time. Unlock the extension and try again.";
  }

  return message || `${wallet?.adapter.name ?? "Wallet"} did not open. Unlock the extension and try again.`;
}

export function WalletControl() {
  const {
    connect,
    connected,
    connecting,
    disconnect,
    disconnecting,
    publicKey,
    wallet,
  } = useWallet();
  const { setVisible } = useWalletModal();
  const [menuOpen, setMenuOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [connectError, setConnectError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: MouseEvent | TouchEvent) => {
      if (rootRef.current?.contains(event.target as Node)) return;
      setMenuOpen(false);
    };

    document.addEventListener("mousedown", close);
    document.addEventListener("touchstart", close);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("touchstart", close);
    };
  }, [menuOpen]);

  useEffect(() => {
    if (!connected) setMenuOpen(false);
    if (connected) setConnectError(null);
  }, [connected]);

  const busy = connecting || disconnecting;
  const selectedWalletReady = canOpenWallet(wallet);
  const buttonLabel = connected && publicKey
    ? shortAddress(publicKey)
    : connecting
      ? "Connecting..."
      : disconnecting
        ? "Disconnecting..."
        : wallet && selectedWalletReady
          ? `Connect ${wallet.adapter.name}`
          : "Connect Wallet";

  const handlePrimaryClick = () => {
    if (busy) return;
    if (connected) {
      setMenuOpen((open) => !open);
      return;
    }

    if (!wallet) {
      setConnectError(null);
      setVisible(true);
      return;
    }

    if (!selectedWalletReady) {
      setConnectError(selectedWalletIssue(wallet));
      setVisible(true);
      return;
    }

    setConnectError(null);
    void connect().catch((cause: unknown) => {
      setConnectError(walletConnectErrorMessage(cause, wallet));
      setVisible(true);
    });
  };

  const handleCopyAddress = () => {
    if (!publicKey) return;
    void navigator.clipboard.writeText(publicKey.toBase58()).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    }).catch(() => undefined);
  };

  const handleChangeWallet = () => {
    setMenuOpen(false);
    setConnectError(null);
    setVisible(true);
  };

  const handleDisconnect = () => {
    setMenuOpen(false);
    setConnectError(null);
    void disconnect().catch(() => undefined);
  };

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-expanded={connected ? menuOpen : undefined}
        aria-haspopup={connected ? "menu" : undefined}
        data-testid="wallet-control-button"
        onClick={handlePrimaryClick}
        className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-70"
        disabled={busy}
      >
        {busy ? (
          <LoaderCircle
            className="h-4 w-4 animate-spin motion-reduce:animate-none"
            aria-hidden="true"
          />
        ) : connected ? (
          <span className="h-2 w-2 rounded-full bg-success-500" aria-hidden="true" />
        ) : (
          <Wallet className="h-4 w-4" aria-hidden="true" />
        )}
        <span className={cn(connected && "font-mono tabular-nums")}>{buttonLabel}</span>
        {connected ? <ChevronDown className="h-4 w-4" aria-hidden="true" /> : null}
      </button>

      {connectError && !connected ? (
        <div
          role="alert"
          className="absolute right-0 top-full z-50 mt-2 w-[min(20rem,calc(100vw-2rem))] rounded-md border border-destructive/30 bg-card p-3 text-sm shadow-xl"
        >
          <p className="font-medium text-foreground">Wallet did not open</p>
          <p className="mt-1 leading-5 text-muted-foreground">{connectError}</p>
          <button
            type="button"
            onClick={handleChangeWallet}
            className="mt-3 inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <RefreshCw className="h-4 w-4" aria-hidden="true" />
            Choose Wallet
          </button>
        </div>
      ) : null}

      <div
        role="menu"
        aria-label="Wallet Actions"
        data-testid="wallet-control-menu"
        className={cn(
          "absolute right-0 top-full z-50 mt-2 min-w-[180px] rounded-md border border-border bg-card p-1 shadow-xl",
          menuOpen ? "block" : "hidden",
        )}
      >
        <button
          type="button"
          role="menuitem"
          onClick={handleCopyAddress}
          className="flex min-h-10 w-full items-center gap-2 rounded-md px-3 text-left text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
        >
          {copied ? <Check className="h-4 w-4" aria-hidden="true" /> : <Copy className="h-4 w-4" aria-hidden="true" />}
          {copied ? "Copied" : "Copy Address"}
        </button>
        <button
          type="button"
          role="menuitem"
          onClick={handleChangeWallet}
          className="flex min-h-10 w-full items-center gap-2 rounded-md px-3 text-left text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
        >
          <RefreshCw className="h-4 w-4" aria-hidden="true" />
          Change Wallet
        </button>
        <button
          type="button"
          role="menuitem"
          onClick={handleDisconnect}
          className="flex min-h-10 w-full items-center gap-2 rounded-md px-3 text-left text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
        >
          <LogOut className="h-4 w-4" aria-hidden="true" />
          Disconnect
        </button>
      </div>
    </div>
  );
}
