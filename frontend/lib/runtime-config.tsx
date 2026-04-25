"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";

import {
  ALLOWED_CONFIGS,
  AllowedClusterConfig,
  defaultClusterId,
  findAllowedConfig,
} from "@/lib/allowed-configs";
import {
  isClusterId,
  RUNTIME_CONFIG_STORAGE_KEY,
} from "@/lib/runtime-config-schema";
import type { ClusterId } from "@/lib/types";

export type GenesisCheckState =
  | { status: "pending" }
  | { status: "ok" }
  | { status: "skipped"; reason: string }
  | { status: "error"; reason: string };

interface RuntimeConfigContextValue {
  cluster: ClusterId;
  /**
   * Begin a cluster switch. Until the user confirms via `confirmClusterChange`,
   * the active cluster is unchanged and a consent modal is shown (rendered
   * by the app shell).
   */
  requestClusterChange: (cluster: ClusterId) => void;
  pendingClusterChange: ClusterId | null;
  confirmClusterChange: () => void;
  cancelClusterChange: () => void;
  current: AllowedClusterConfig;
  allowedConfigs: AllowedClusterConfig[];
  genesisCheck: GenesisCheckState;
  retryGenesisCheck: () => void;
}

const RuntimeConfigContext = createContext<RuntimeConfigContextValue | null>(null);

function readStoredClusterId(): ClusterId | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(RUNTIME_CONFIG_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as unknown;
    // Audit F3 — read ONLY the `cluster` field. Any other fields in the
    // stored blob are ignored. A poisoned localStorage value can at worst
    // choose among the three allowlisted clusters.
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      "cluster" in parsed &&
      isClusterId((parsed as { cluster: unknown }).cluster)
    ) {
      const id = (parsed as { cluster: ClusterId }).cluster;
      if (findAllowedConfig(id)) return id;
    }
  } catch {
    // Stale or corrupt storage — fall through to default.
  }
  return null;
}

async function fetchGenesisHash(rpcUrl: string, signal: AbortSignal): Promise<string> {
  const response = await fetch(rpcUrl, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "getGenesisHash",
    }),
    signal,
  });
  if (!response.ok) {
    throw new Error(`RPC returned ${response.status}`);
  }
  const body = await response.json();
  if (body?.error) {
    throw new Error(`RPC error: ${body.error.message ?? "unknown"}`);
  }
  const hash = body?.result;
  if (typeof hash !== "string" || !hash) {
    throw new Error("RPC did not return a genesis hash");
  }
  return hash;
}

export function RuntimeConfigProvider({ children }: { children: React.ReactNode }) {
  const [cluster, setCluster] = useState<ClusterId>(() => defaultClusterId());
  const [pendingClusterChange, setPendingClusterChange] = useState<ClusterId | null>(null);
  // Lazy initial state: if the default cluster has no pinned genesisHash
  // (localnet / unpinned dev) the check is skipped from the first render,
  // so the runtime-gate banner doesn't flash on mount. For clusters WITH
  // a pinned hash we start in "pending" and the effect below runs the
  // real verification.
  const [genesisCheck, setGenesisCheck] = useState<GenesisCheckState>(() => {
    const initial = findAllowedConfig(defaultClusterId());
    if (initial && !initial.genesisHash) {
      return {
        status: "skipped",
        reason: "cluster has no pinned genesis hash (localnet or custom)",
      };
    }
    return { status: "pending" };
  });
  const [genesisCheckNonce, setGenesisCheckNonce] = useState(0);
  const [storageHydrated, setStorageHydrated] = useState(false);

  // Hydrate cluster id from localStorage AFTER the first render to avoid a
  // server/client hydration mismatch — localStorage is only available on
  // the client.
  useEffect(() => {
    const stored = readStoredClusterId();
    if (stored) setCluster(stored);
    setStorageHydrated(true);
  }, []);

  // Persist ONLY the cluster id (audit F3).
  useEffect(() => {
    if (typeof window === "undefined") return;
    if (!storageHydrated) return;
    window.localStorage.setItem(
      RUNTIME_CONFIG_STORAGE_KEY,
      JSON.stringify({ cluster }),
    );
  }, [cluster, storageHydrated]);

  const current = useMemo<AllowedClusterConfig>(() => {
    return (
      findAllowedConfig(cluster) ??
      findAllowedConfig(defaultClusterId()) ??
      ALLOWED_CONFIGS[0]
    );
  }, [cluster]);

  // Genesis-hash verification (audit F3). Fires whenever `current.rpcUrl`
  // changes. Localnet and clusters whose genesisHash is empty are
  // explicitly skipped — localnet's genesis is ephemeral and the caller
  // opted out by leaving the field empty.
  useEffect(() => {
    if (!current.genesisHash) {
      setGenesisCheck({
        status: "skipped",
        reason: "cluster has no pinned genesis hash (localnet or custom)",
      });
      return;
    }
    setGenesisCheck({ status: "pending" });
    const controller = new AbortController();
    fetchGenesisHash(current.rpcUrl, controller.signal)
      .then((observed) => {
        if (observed !== current.genesisHash) {
          setGenesisCheck({
            status: "error",
            reason: `RPC genesis ${observed.slice(0, 12)}… does not match the expected ${current.genesisHash.slice(0, 12)}…. The endpoint is not on the ${current.id} cluster.`,
          });
          return;
        }
        setGenesisCheck({ status: "ok" });
      })
      .catch((err: unknown) => {
        if (controller.signal.aborted) return;
        const message = err instanceof Error ? err.message : String(err);
        setGenesisCheck({
          status: "error",
          reason: `Could not verify cluster genesis: ${message}`,
        });
      });
    return () => controller.abort();
  }, [current.rpcUrl, current.genesisHash, current.id, genesisCheckNonce]);

  const requestClusterChange = useCallback(
    (next: ClusterId) => {
      if (!findAllowedConfig(next)) return;
      if (next === cluster) return;
      setPendingClusterChange(next);
    },
    [cluster],
  );

  const confirmClusterChange = useCallback(() => {
    if (pendingClusterChange && findAllowedConfig(pendingClusterChange)) {
      setCluster(pendingClusterChange);
    }
    setPendingClusterChange(null);
  }, [pendingClusterChange]);

  const cancelClusterChange = useCallback(() => {
    setPendingClusterChange(null);
  }, []);

  const retryGenesisCheck = useCallback(() => {
    setGenesisCheckNonce((n) => n + 1);
  }, []);

  const value = useMemo<RuntimeConfigContextValue>(
    () => ({
      cluster,
      requestClusterChange,
      pendingClusterChange,
      confirmClusterChange,
      cancelClusterChange,
      current,
      allowedConfigs: ALLOWED_CONFIGS,
      genesisCheck,
      retryGenesisCheck,
    }),
    [
      cluster,
      requestClusterChange,
      pendingClusterChange,
      confirmClusterChange,
      cancelClusterChange,
      current,
      genesisCheck,
      retryGenesisCheck,
    ],
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
