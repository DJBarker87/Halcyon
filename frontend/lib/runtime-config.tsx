"use client";

import { createContext, useContext, useEffect, useMemo, useState } from "react";

import {
  isClusterId,
  isFieldLocked,
  overrideKeysForCluster,
  RUNTIME_CONFIG_STORAGE_KEY,
  sanitizeStoredClusterConfig,
} from "@/lib/runtime-config-schema";
import type { ClusterConfig, ClusterId } from "@/lib/types";

const DEFAULT_LOCALNET_IDS = {
  kernelProgramId: "H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF",
  flagshipProgramId: "E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc",
  ilProtectionProgramId: "HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g",
  solAutocallProgramId: "6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8",
};

function env(cluster: ClusterId, key: string) {
  const suffix = cluster === "mainnet" ? "MAINNET" : cluster.toUpperCase();
  return process.env[`NEXT_PUBLIC_${key}_${suffix}`] ?? "";
}

function makeDefaults(cluster: ClusterId): ClusterConfig {
  return {
    rpcUrl:
      env(cluster, "RPC_URL") ||
      (cluster === "localnet"
        ? "http://127.0.0.1:8899"
        : cluster === "devnet"
          ? "https://api.devnet.solana.com"
          : "https://api.mainnet-beta.solana.com"),
    kernelProgramId:
      env(cluster, "KERNEL_PROGRAM_ID") ||
      (cluster === "localnet" ? DEFAULT_LOCALNET_IDS.kernelProgramId : ""),
    flagshipProgramId:
      env(cluster, "FLAGSHIP_PROGRAM_ID") ||
      (cluster === "localnet" ? DEFAULT_LOCALNET_IDS.flagshipProgramId : ""),
    ilProtectionProgramId:
      env(cluster, "IL_PROGRAM_ID") ||
      (cluster === "localnet" ? DEFAULT_LOCALNET_IDS.ilProtectionProgramId : ""),
    solAutocallProgramId:
      env(cluster, "SOL_PROGRAM_ID") ||
      (cluster === "localnet" ? DEFAULT_LOCALNET_IDS.solAutocallProgramId : ""),
    pythSol: env(cluster, "PYTH_SOL_ACCOUNT"),
    pythUsdc: env(cluster, "PYTH_USDC_ACCOUNT"),
    pythSpy: env(cluster, "PYTH_SPY_ACCOUNT"),
    pythQqq: env(cluster, "PYTH_QQQ_ACCOUNT"),
    pythIwm: env(cluster, "PYTH_IWM_ACCOUNT"),
  };
}

interface RuntimeConfigContextValue {
  cluster: ClusterId;
  setCluster: (cluster: ClusterId) => void;
  current: ClusterConfig;
  settings: Record<ClusterId, ClusterConfig>;
  currentHasOverrides: boolean;
  currentOverrideKeys: Array<keyof ClusterConfig>;
  isFieldLocked: (key: keyof ClusterConfig) => boolean;
  updateCurrent: (key: keyof ClusterConfig, value: string) => void;
  resetCurrent: () => void;
}

const RuntimeConfigContext = createContext<RuntimeConfigContextValue | null>(null);

export function RuntimeConfigProvider({ children }: { children: React.ReactNode }) {
  const [cluster, setCluster] = useState<ClusterId>("localnet");
  const [settings, setSettings] = useState<Record<ClusterId, ClusterConfig>>({
    localnet: makeDefaults("localnet"),
    devnet: makeDefaults("devnet"),
    mainnet: makeDefaults("mainnet"),
  });
  const defaults = useMemo<Record<ClusterId, ClusterConfig>>(
    () => ({
      localnet: makeDefaults("localnet"),
      devnet: makeDefaults("devnet"),
      mainnet: makeDefaults("mainnet"),
    }),
    [],
  );

  useEffect(() => {
    const raw = window.localStorage.getItem(RUNTIME_CONFIG_STORAGE_KEY);
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw) as {
        cluster?: unknown;
        settings?: Partial<Record<ClusterId, Partial<ClusterConfig>>>;
      };
      if (isClusterId(parsed.cluster)) {
        setCluster(parsed.cluster);
      }
      if (parsed.settings) {
        setSettings((current) => ({
          localnet: {
            ...current.localnet,
            ...sanitizeStoredClusterConfig("localnet", parsed.settings?.localnet),
          },
          devnet: {
            ...current.devnet,
            ...sanitizeStoredClusterConfig("devnet", parsed.settings?.devnet),
          },
          mainnet: {
            ...current.mainnet,
            ...sanitizeStoredClusterConfig("mainnet", parsed.settings?.mainnet),
          },
        }));
      }
    } catch {
      // ignore stale local storage
    }
  }, [defaults]);

  useEffect(() => {
    window.localStorage.setItem(
      RUNTIME_CONFIG_STORAGE_KEY,
      JSON.stringify({
        cluster,
        settings: {
          localnet: sanitizeStoredClusterConfig("localnet", settings.localnet),
          devnet: sanitizeStoredClusterConfig("devnet", settings.devnet),
          mainnet: sanitizeStoredClusterConfig("mainnet", settings.mainnet),
        },
      }),
    );
  }, [cluster, settings]);

  const value = useMemo<RuntimeConfigContextValue>(
    () => ({
      cluster,
      setCluster,
      current: settings[cluster],
      settings,
      currentOverrideKeys: overrideKeysForCluster(cluster, settings[cluster], defaults[cluster]),
      currentHasOverrides: overrideKeysForCluster(cluster, settings[cluster], defaults[cluster]).length > 0,
      isFieldLocked: (key) => isFieldLocked(cluster, key),
      updateCurrent: (key, value) =>
        setSettings((current) => {
          if (isFieldLocked(cluster, key)) {
            return current;
          }
          return {
            ...current,
            [cluster]: {
              ...current[cluster],
              [key]: value,
            },
          };
        }),
      resetCurrent: () =>
        setSettings((current) => ({
          ...current,
          [cluster]: defaults[cluster],
        })),
    }),
    [cluster, defaults, settings],
  );

  return <RuntimeConfigContext.Provider value={value}>{children}</RuntimeConfigContext.Provider>;
}

export function useRuntimeConfig() {
  const context = useContext(RuntimeConfigContext);
  if (!context) {
    throw new Error("useRuntimeConfig must be used within RuntimeConfigProvider");
  }
  return context;
}
