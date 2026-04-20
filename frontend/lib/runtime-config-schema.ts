import { PublicKey } from "@solana/web3.js";

import type { ClusterConfig, ClusterId } from "@/lib/types";

export const RUNTIME_CONFIG_STORAGE_KEY = "halcyon-layer5-runtime-config-v1";

export const CLUSTER_IDS: ClusterId[] = ["localnet", "devnet", "mainnet"];

export const CLUSTER_CONFIG_FIELDS: Array<keyof ClusterConfig> = [
  "rpcUrl",
  "kernelProgramId",
  "flagshipProgramId",
  "solAutocallProgramId",
  "ilProtectionProgramId",
  "pythSol",
  "pythUsdc",
  "pythSpy",
  "pythQqq",
  "pythIwm",
];

const MAINNET_EDITABLE_FIELDS = new Set<keyof ClusterConfig>(["rpcUrl"]);

export function isClusterId(value: unknown): value is ClusterId {
  return typeof value === "string" && CLUSTER_IDS.includes(value as ClusterId);
}

export function isFieldLocked(cluster: ClusterId, key: keyof ClusterConfig) {
  return cluster === "mainnet" && !MAINNET_EDITABLE_FIELDS.has(key);
}

export function normalizeRpcUrl(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return "";
  try {
    const url = new URL(trimmed);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return null;
    }
    return url.toString();
  } catch {
    return null;
  }
}

export function normalizePubkey(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return "";
  try {
    return new PublicKey(trimmed).toBase58();
  } catch {
    return null;
  }
}

export function isValidRuntimeConfigValue(key: keyof ClusterConfig, value: string) {
  return key === "rpcUrl" ? normalizeRpcUrl(value) !== null : normalizePubkey(value) !== null;
}

export function sanitizeStoredClusterConfig(
  cluster: ClusterId,
  config: Partial<ClusterConfig> | undefined,
) {
  const sanitized: Partial<ClusterConfig> = {};
  if (!config) return sanitized;

  for (const key of CLUSTER_CONFIG_FIELDS) {
    const rawValue = config[key];
    if (typeof rawValue !== "string") continue;
    if (isFieldLocked(cluster, key)) continue;

    const normalized =
      key === "rpcUrl" ? normalizeRpcUrl(rawValue) : normalizePubkey(rawValue);
    if (normalized === null) continue;
    sanitized[key] = normalized;
  }

  return sanitized;
}

export function overrideKeysForCluster(cluster: ClusterId, current: ClusterConfig, defaults: ClusterConfig) {
  return CLUSTER_CONFIG_FIELDS.filter((key) => {
    const currentValue = current[key].trim();
    const defaultValue = defaults[key].trim();
    if (isFieldLocked(cluster, key)) {
      return key === "rpcUrl" && currentValue !== defaultValue;
    }
    return currentValue !== defaultValue;
  });
}
