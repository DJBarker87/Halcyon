import type { PublicKey } from "@solana/web3.js";
import { BN } from "@coral-xyz/anchor";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function isBn(value: unknown): value is BN {
  return value instanceof BN || BN.isBN(value) || isBnLike(value);
}

function isBnLike(value: unknown): value is { toNumber?: () => number; toString: () => string } {
  const candidate = value as {
    length?: unknown;
    negative?: unknown;
    toString?: unknown;
    words?: unknown;
  };
  return (
    !!value &&
    typeof value === "object" &&
    typeof candidate.negative === "number" &&
    typeof candidate.length === "number" &&
    typeof candidate.words === "object" &&
    candidate.words !== null &&
    typeof candidate.toString === "function"
  );
}

export function toNumber(value: unknown): number {
  if (typeof value === "number") return value;
  if (typeof value === "bigint") return Number(value);
  if (isBn(value)) {
    if (typeof value.toNumber === "function") return value.toNumber();
    return Number(value.toString());
  }
  if (typeof value === "string") return Number(value);
  return 0;
}

export function toStringValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number") return String(value);
  if (typeof value === "bigint") return value.toString();
  if (isBn(value)) return value.toString();
  if (value && typeof value === "object" && "toBase58" in value) {
    return (value as PublicKey).toBase58();
  }
  return "";
}

export function formatUsdcBaseUnits(value: unknown, maximumFractionDigits = 2) {
  const amount = toNumber(value) / 1_000_000;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 0,
    maximumFractionDigits,
  }).format(amount);
}

export function formatPercentFromBpsS6(value: unknown, multiplier = 1) {
  const amount = (toNumber(value) / 1_000_000 / 10_000) * 100 * multiplier;
  return `${amount.toFixed(amount >= 10 ? 2 : 3)}%`;
}

export function formatPercentFromS6(value: unknown) {
  const amount = (toNumber(value) / 1_000_000) * 100;
  return `${amount.toFixed(amount >= 10 ? 2 : 3)}%`;
}

export function formatRatio(value: number) {
  return `${(value * 100).toFixed(1)}%`;
}

export function shortAddress(value: PublicKey | string | null | undefined, chars = 4) {
  if (!value) return "Not set";
  const base58 = typeof value === "string" ? value : value.toBase58();
  if (base58.length <= chars * 2 + 3) return base58;
  return `${base58.slice(0, chars)}...${base58.slice(-chars)}`;
}

export function toBaseUnits(input: string) {
  const normalized = input.trim();
  if (!normalized) return new BN(0);
  const [whole, fraction = ""] = normalized.split(".");
  const wholeBn = new BN(whole || "0").mul(new BN(1_000_000));
  const paddedFraction = `${fraction}000000`.slice(0, 6);
  return wholeBn.add(new BN(paddedFraction || "0"));
}

export function enumTag(value: unknown) {
  if (!value) return "unknown";
  if (typeof value === "string") return value;
  if (typeof value === "object") {
    const keys = Object.keys(value as Record<string, unknown>);
    return keys[0] ?? "unknown";
  }
  return String(value);
}

export function field<T = unknown>(
  record: Record<string, unknown>,
  camelKey: string,
  snakeKey?: string,
): T | undefined {
  const snake = snakeKey ?? camelKey.replace(/[A-Z]/g, (match) => `_${match.toLowerCase()}`);
  return (record[camelKey] ?? record[snake]) as T | undefined;
}
