/**
 * Compile-time allowlist of runtime configurations (audit Finding 3).
 *
 * Before this change, the frontend merged arbitrary fields from
 * localStorage into runtime config. A poisoned browser state could
 * rewire RPC endpoint, program IDs, and oracle accounts — and the wallet
 * would happily auto-connect and build transactions against those
 * values.
 *
 * After this change:
 *
 *   - Only the cluster `id` is read from localStorage.
 *   - Every other field (RPC, program IDs, Pyth accounts, genesis hash)
 *     is resolved from the allowlist below, which is baked into the
 *     production bundle at build time via NEXT_PUBLIC_* env vars.
 *   - If the stored id is not in the allowlist, the app falls back to
 *     the default cluster (mainnet in production, devnet in dev).
 *   - Localnet is present only when NODE_ENV === 'development' so it
 *     cannot leak into a production bundle (checked by the Playwright
 *     build-guard test).
 *
 * Genesis hashes are the canonical public values. They are used at load
 * time to confirm the selected RPC is actually on the cluster the
 * allowlist claims it is; a mismatch refuses the wallet connection and
 * surfaces an error banner.
 */

import type { ClusterConfig, ClusterId } from "@/lib/types";

export interface AllowedClusterConfig extends ClusterConfig {
  id: ClusterId;
  label: string;
  /**
   * Base58 Solana genesis hash for the cluster. Empty for localnet
   * (ephemeral per-session). Production paths require a non-empty value.
   */
  genesisHash: string;
  /** User-facing explanation shown in the cluster-switch consent modal. */
  description: string;
}

export const SOLANA_GENESIS_HASHES = {
  "mainnet-beta": "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d",
  devnet: "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG",
  testnet: "4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY",
} as const;

// Next.js only inlines `process.env.NEXT_PUBLIC_*` when the access is a
// literal property read. A computed read (`process.env[x]`) is not inlined
// and resolves to undefined in the client bundle. This map materializes
// every literal access so the `env()` lookup below is an object read on
// values that webpack has already baked in at build time.
const PUBLIC_ENV: Readonly<Record<string, string | undefined>> = {
  NEXT_PUBLIC_RPC_URL_MAINNET: process.env.NEXT_PUBLIC_RPC_URL_MAINNET,
  NEXT_PUBLIC_RPC_URL_DEVNET: process.env.NEXT_PUBLIC_RPC_URL_DEVNET,
  NEXT_PUBLIC_RPC_URL_LOCALNET: process.env.NEXT_PUBLIC_RPC_URL_LOCALNET,
  NEXT_PUBLIC_KERNEL_PROGRAM_ID_MAINNET: process.env.NEXT_PUBLIC_KERNEL_PROGRAM_ID_MAINNET,
  NEXT_PUBLIC_KERNEL_PROGRAM_ID_DEVNET: process.env.NEXT_PUBLIC_KERNEL_PROGRAM_ID_DEVNET,
  NEXT_PUBLIC_KERNEL_PROGRAM_ID_LOCALNET: process.env.NEXT_PUBLIC_KERNEL_PROGRAM_ID_LOCALNET,
  NEXT_PUBLIC_FLAGSHIP_PROGRAM_ID_MAINNET: process.env.NEXT_PUBLIC_FLAGSHIP_PROGRAM_ID_MAINNET,
  NEXT_PUBLIC_FLAGSHIP_PROGRAM_ID_DEVNET: process.env.NEXT_PUBLIC_FLAGSHIP_PROGRAM_ID_DEVNET,
  NEXT_PUBLIC_FLAGSHIP_PROGRAM_ID_LOCALNET: process.env.NEXT_PUBLIC_FLAGSHIP_PROGRAM_ID_LOCALNET,
  NEXT_PUBLIC_IL_PROGRAM_ID_MAINNET: process.env.NEXT_PUBLIC_IL_PROGRAM_ID_MAINNET,
  NEXT_PUBLIC_IL_PROGRAM_ID_DEVNET: process.env.NEXT_PUBLIC_IL_PROGRAM_ID_DEVNET,
  NEXT_PUBLIC_IL_PROGRAM_ID_LOCALNET: process.env.NEXT_PUBLIC_IL_PROGRAM_ID_LOCALNET,
  NEXT_PUBLIC_SOL_PROGRAM_ID_MAINNET: process.env.NEXT_PUBLIC_SOL_PROGRAM_ID_MAINNET,
  NEXT_PUBLIC_SOL_PROGRAM_ID_DEVNET: process.env.NEXT_PUBLIC_SOL_PROGRAM_ID_DEVNET,
  NEXT_PUBLIC_SOL_PROGRAM_ID_LOCALNET: process.env.NEXT_PUBLIC_SOL_PROGRAM_ID_LOCALNET,
  NEXT_PUBLIC_PYTH_SOL_ACCOUNT_MAINNET: process.env.NEXT_PUBLIC_PYTH_SOL_ACCOUNT_MAINNET,
  NEXT_PUBLIC_PYTH_SOL_ACCOUNT_DEVNET: process.env.NEXT_PUBLIC_PYTH_SOL_ACCOUNT_DEVNET,
  NEXT_PUBLIC_PYTH_SOL_ACCOUNT_LOCALNET: process.env.NEXT_PUBLIC_PYTH_SOL_ACCOUNT_LOCALNET,
  NEXT_PUBLIC_PYTH_USDC_ACCOUNT_MAINNET: process.env.NEXT_PUBLIC_PYTH_USDC_ACCOUNT_MAINNET,
  NEXT_PUBLIC_PYTH_USDC_ACCOUNT_DEVNET: process.env.NEXT_PUBLIC_PYTH_USDC_ACCOUNT_DEVNET,
  NEXT_PUBLIC_PYTH_USDC_ACCOUNT_LOCALNET: process.env.NEXT_PUBLIC_PYTH_USDC_ACCOUNT_LOCALNET,
  NEXT_PUBLIC_PYTH_SPY_ACCOUNT_MAINNET: process.env.NEXT_PUBLIC_PYTH_SPY_ACCOUNT_MAINNET,
  NEXT_PUBLIC_PYTH_SPY_ACCOUNT_DEVNET: process.env.NEXT_PUBLIC_PYTH_SPY_ACCOUNT_DEVNET,
  NEXT_PUBLIC_PYTH_SPY_ACCOUNT_LOCALNET: process.env.NEXT_PUBLIC_PYTH_SPY_ACCOUNT_LOCALNET,
  NEXT_PUBLIC_PYTH_QQQ_ACCOUNT_MAINNET: process.env.NEXT_PUBLIC_PYTH_QQQ_ACCOUNT_MAINNET,
  NEXT_PUBLIC_PYTH_QQQ_ACCOUNT_DEVNET: process.env.NEXT_PUBLIC_PYTH_QQQ_ACCOUNT_DEVNET,
  NEXT_PUBLIC_PYTH_QQQ_ACCOUNT_LOCALNET: process.env.NEXT_PUBLIC_PYTH_QQQ_ACCOUNT_LOCALNET,
  NEXT_PUBLIC_PYTH_IWM_ACCOUNT_MAINNET: process.env.NEXT_PUBLIC_PYTH_IWM_ACCOUNT_MAINNET,
  NEXT_PUBLIC_PYTH_IWM_ACCOUNT_DEVNET: process.env.NEXT_PUBLIC_PYTH_IWM_ACCOUNT_DEVNET,
  NEXT_PUBLIC_PYTH_IWM_ACCOUNT_LOCALNET: process.env.NEXT_PUBLIC_PYTH_IWM_ACCOUNT_LOCALNET,
};

function env(cluster: ClusterId, key: string) {
  const suffix = cluster === "mainnet" ? "MAINNET" : cluster.toUpperCase();
  return PUBLIC_ENV[`NEXT_PUBLIC_${key}_${suffix}`] ?? "";
}

// Hardcoded localnet program IDs — these are the defaults shipped with
// Anchor.toml, identical to the pre-audit `DEFAULT_LOCALNET_IDS`. They
// matter only when NODE_ENV === 'development'; a production bundle
// excludes the localnet entry entirely (see `buildAllowedConfigs` below).
const LOCALNET_PROGRAM_IDS = {
  kernelProgramId: "H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF",
  flagshipProgramId: "E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc",
  ilProtectionProgramId: "HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g",
  solAutocallProgramId: "6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8",
};

function localnetConfig(): AllowedClusterConfig {
  return {
    id: "localnet",
    label: "Localnet (development only)",
    description:
      "Solana test validator on 127.0.0.1. For local development only; the genesis is ephemeral so the on-load genesis-hash check is skipped.",
    rpcUrl: env("localnet", "RPC_URL") || "http://127.0.0.1:8899",
    kernelProgramId:
      env("localnet", "KERNEL_PROGRAM_ID") || LOCALNET_PROGRAM_IDS.kernelProgramId,
    flagshipProgramId:
      env("localnet", "FLAGSHIP_PROGRAM_ID") || LOCALNET_PROGRAM_IDS.flagshipProgramId,
    ilProtectionProgramId:
      env("localnet", "IL_PROGRAM_ID") || LOCALNET_PROGRAM_IDS.ilProtectionProgramId,
    solAutocallProgramId:
      env("localnet", "SOL_PROGRAM_ID") || LOCALNET_PROGRAM_IDS.solAutocallProgramId,
    pythSol: env("localnet", "PYTH_SOL_ACCOUNT"),
    pythUsdc: env("localnet", "PYTH_USDC_ACCOUNT"),
    pythSpy: env("localnet", "PYTH_SPY_ACCOUNT"),
    pythQqq: env("localnet", "PYTH_QQQ_ACCOUNT"),
    pythIwm: env("localnet", "PYTH_IWM_ACCOUNT"),
    genesisHash: "",
  };
}

function devnetConfig(): AllowedClusterConfig {
  return {
    id: "devnet",
    label: "Devnet",
    description:
      "Solana devnet. Not real funds; safe for testing. Switching here from mainnet means a different set of pinned program IDs and oracle feeds.",
    rpcUrl: env("devnet", "RPC_URL") || "https://api.devnet.solana.com",
    kernelProgramId: env("devnet", "KERNEL_PROGRAM_ID"),
    flagshipProgramId: env("devnet", "FLAGSHIP_PROGRAM_ID"),
    ilProtectionProgramId: env("devnet", "IL_PROGRAM_ID"),
    solAutocallProgramId: env("devnet", "SOL_PROGRAM_ID"),
    pythSol: env("devnet", "PYTH_SOL_ACCOUNT"),
    pythUsdc: env("devnet", "PYTH_USDC_ACCOUNT"),
    pythSpy: env("devnet", "PYTH_SPY_ACCOUNT"),
    pythQqq: env("devnet", "PYTH_QQQ_ACCOUNT"),
    pythIwm: env("devnet", "PYTH_IWM_ACCOUNT"),
    genesisHash: SOLANA_GENESIS_HASHES.devnet,
  };
}

function mainnetConfig(): AllowedClusterConfig {
  return {
    id: "mainnet",
    label: "Mainnet",
    description:
      "Solana mainnet-beta. Real funds, production protocol. Any transaction built here executes on the live network.",
    rpcUrl: env("mainnet", "RPC_URL") || "https://api.mainnet-beta.solana.com",
    kernelProgramId: env("mainnet", "KERNEL_PROGRAM_ID"),
    flagshipProgramId: env("mainnet", "FLAGSHIP_PROGRAM_ID"),
    ilProtectionProgramId: env("mainnet", "IL_PROGRAM_ID"),
    solAutocallProgramId: env("mainnet", "SOL_PROGRAM_ID"),
    pythSol: env("mainnet", "PYTH_SOL_ACCOUNT"),
    pythUsdc: env("mainnet", "PYTH_USDC_ACCOUNT"),
    pythSpy: env("mainnet", "PYTH_SPY_ACCOUNT"),
    pythQqq: env("mainnet", "PYTH_QQQ_ACCOUNT"),
    pythIwm: env("mainnet", "PYTH_IWM_ACCOUNT"),
    genesisHash: SOLANA_GENESIS_HASHES["mainnet-beta"],
  };
}

/**
 * Build the full allowlist. Localnet is included only when
 * NODE_ENV === 'development'; a production bundle omits it so a
 * post-deploy grep for localhost / localnet program IDs returns no hits.
 */
export function buildAllowedConfigs(): AllowedClusterConfig[] {
  const configs: AllowedClusterConfig[] = [mainnetConfig(), devnetConfig()];
  if (process.env.NODE_ENV === "development") {
    configs.unshift(localnetConfig());
  }
  return configs;
}

export const ALLOWED_CONFIGS: AllowedClusterConfig[] = buildAllowedConfigs();

export function findAllowedConfig(id: string): AllowedClusterConfig | undefined {
  return ALLOWED_CONFIGS.find((c) => c.id === id);
}

/** Default cluster when none is stored or the stored id is not in the allowlist. */
export function defaultClusterId(): ClusterId {
  if (process.env.NODE_ENV === "development") return "localnet";
  return "mainnet";
}

/** Subset of fields that a user-facing settings panel displays read-only. */
export const READ_ONLY_DISPLAY_FIELDS: Array<{ key: keyof ClusterConfig; label: string }> = [
  { key: "rpcUrl", label: "RPC URL" },
  { key: "kernelProgramId", label: "Kernel program" },
  { key: "flagshipProgramId", label: "Flagship program" },
  { key: "ilProtectionProgramId", label: "IL Protection program" },
  { key: "solAutocallProgramId", label: "SOL Autocall program" },
  { key: "pythSpy", label: "SPY price account" },
  { key: "pythQqq", label: "QQQ price account" },
  { key: "pythIwm", label: "IWM price account" },
  { key: "pythSol", label: "SOL price account" },
  { key: "pythUsdc", label: "USDC price account" },
];
