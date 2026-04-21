import type { ClusterId } from "@/lib/types";

/**
 * localStorage key for the persisted cluster selection. The bumped
 * version suffix distinguishes the F3-hardened payload (cluster id only)
 * from the pre-audit shape that merged arbitrary per-field overrides.
 */
export const RUNTIME_CONFIG_STORAGE_KEY = "halcyon-layer5-runtime-config-v2";

export const CLUSTER_IDS: ClusterId[] = ["localnet", "devnet", "mainnet"];

export function isClusterId(value: unknown): value is ClusterId {
  return typeof value === "string" && CLUSTER_IDS.includes(value as ClusterId);
}
