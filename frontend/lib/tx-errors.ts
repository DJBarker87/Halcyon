/**
 * Translate wallet / RPC / Anchor errors into buyer-friendly messages.
 *
 * Routes used to surface `previewError` / `issueError` as raw strings. That
 * meant a user saw things like `0x1770` or `Transaction simulation failed:
 * Blockhash not found`. Those tell the builder what happened and the buyer
 * nothing.
 *
 * `mapSolanaError(err)` returns a `{ title, body, retryable }` triple that
 * the UI renders as a warm error block. Unknown errors pass through with
 * the raw message in `body` so we never fully hide information — we just
 * cap the noise.
 */

import flagshipIdlJson from "../../target/idl/halcyon_flagship_autocall.json";
import ilIdlJson from "../../target/idl/halcyon_il_protection.json";
import kernelIdlJson from "../../target/idl/halcyon_kernel.json";
import solIdlJson from "../../target/idl/halcyon_sol_autocall.json";

type IdlError = { code: number; name: string; msg: string };
type IdlWithErrors = { errors?: IdlError[]; metadata?: { name?: string } };

const IDLS: IdlWithErrors[] = [
  kernelIdlJson as IdlWithErrors,
  flagshipIdlJson as IdlWithErrors,
  ilIdlJson as IdlWithErrors,
  solIdlJson as IdlWithErrors,
];

type AnchorErrorEntry = { name: string; msg: string; program: string };

// Per-program error tables, keyed by code.
const ERROR_BY_PROGRAM: Map<string, Map<number, AnchorErrorEntry>> = new Map();
// A fallback "seen anywhere" table keyed by code — if we can't determine
// which program produced the error, we fall through this.
const ERROR_ANY: Map<number, AnchorErrorEntry> = new Map();

for (const idl of IDLS) {
  const program = idl.metadata?.name ?? "unknown";
  const table = new Map<number, AnchorErrorEntry>();
  for (const err of idl.errors ?? []) {
    const entry: AnchorErrorEntry = { name: err.name, msg: err.msg, program };
    table.set(err.code, entry);
    if (!ERROR_ANY.has(err.code)) ERROR_ANY.set(err.code, entry);
  }
  ERROR_BY_PROGRAM.set(program, table);
}

export type MappedError = {
  /** One-line summary shown as the error block's heading. */
  title: string;
  /** Plain-English explanation + what the user can do about it. */
  body: string;
  /** Hint for whether the UI should show a retry button. */
  retryable: boolean;
  /** Optional detail the user can expand to see (raw message). */
  detail?: string;
};

// ─── Public API ─────────────────────────────────────────────────────

export function mapSolanaError(err: unknown): MappedError {
  const raw = errorText(err);
  const logs = errorLogs(err);

  // 1. User rejected signing.
  if (isUserRejection(err, raw)) {
    return {
      title: "You cancelled signing.",
      body: "No transaction was sent. Click the button again when you're ready.",
      retryable: true,
    };
  }

  // 2. Insufficient lamports for fees / rent.
  if (isInsufficientFunds(raw, logs)) {
    return {
      title: "Not enough SOL for network fees.",
      body: "Top up your wallet with a small amount of SOL (~0.01) and try again. Devnet SOL is free via `solana airdrop`.",
      retryable: true,
    };
  }

  // 3. Blockhash expired / not found.
  if (isBlockhashExpired(raw, logs)) {
    return {
      title: "The quote expired before the transaction landed.",
      body: "Refresh the quote and try again — Solana drops transactions whose blockhash is too old.",
      retryable: true,
    };
  }

  // 4. RPC unreachable / network hiccup.
  if (isNetworkError(raw)) {
    return {
      title: "Couldn't reach the network.",
      body: "The Solana RPC endpoint didn't respond. Check your connection or switch cluster in Network settings.",
      retryable: true,
    };
  }

  // 5. Simulation failed — generic wrapper, often hides the real reason below.
  // 6. Anchor custom error code.
  const anchorError = extractAnchorError(err, raw, logs);
  if (anchorError) {
    const translated = translateAnchorError(anchorError);
    return {
      title: translated.title,
      body: translated.body,
      retryable: translated.retryable,
      detail: `${anchorError.program}: ${anchorError.name} (code ${anchorError.code})`,
    };
  }

  // 7. Anchor return-data decode failed — usually means the program didn't
  // return what the client expected (version skew).
  if (raw.includes("returned no Anchor return data") || raw.includes("decoding Anchor return data")) {
    return {
      title: "Couldn't read the program's response.",
      body: "The on-chain program returned something this client can't decode. You may need to refresh the page or ensure the frontend matches the deployed program version.",
      retryable: true,
    };
  }

  // 8. Wallet not connected.
  if (raw.includes("Wallet not connected") || raw.includes("No wallet selected")) {
    return {
      title: "Connect a wallet first.",
      body: "Use the Connect button and pick the wallet you want to sign with.",
      retryable: true,
    };
  }

  // 9. Fallback — preserve the raw message so we never swallow information.
  return {
    title: "Something went wrong.",
    body: raw || "An unexpected error occurred. Check the browser console for details.",
    retryable: true,
  };
}

// ─── Detection helpers ──────────────────────────────────────────────

function errorText(err: unknown): string {
  if (err == null) return "";
  if (typeof err === "string") return err;
  if (err instanceof Error) {
    // WalletSendTransactionError from wallet-adapter packs the original in
    // `.error` sometimes; surface both.
    const extra = (err as { error?: unknown }).error;
    const extraMsg = extra && extra !== err ? ` ${errorText(extra)}` : "";
    return `${err.message}${extraMsg}`;
  }
  if (typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message ?? "");
  }
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

function errorLogs(err: unknown): string[] {
  if (!err || typeof err !== "object") return [];
  const maybeLogs = (err as { logs?: unknown }).logs;
  if (Array.isArray(maybeLogs)) return maybeLogs.filter((l): l is string => typeof l === "string");
  // SendTransactionError from @solana/web3.js exposes a getLogs helper.
  const maybeGetLogs = (err as { getLogs?: () => Promise<string[]> | string[] }).getLogs;
  if (typeof maybeGetLogs === "function") {
    try {
      const result = maybeGetLogs.call(err);
      if (Array.isArray(result)) return result;
    } catch {
      // Ignore — getLogs may require a connection arg we don't have here.
    }
  }
  return [];
}

function isUserRejection(err: unknown, raw: string): boolean {
  // Standard EIP-1193-style code used by many wallets.
  const code = err && typeof err === "object" ? (err as { code?: unknown }).code : undefined;
  if (code === 4001 || code === "4001") return true;
  const needles = [
    "user rejected",
    "user denied",
    "rejected the request",
    "transaction cancelled",
    "transaction canceled",
    "wallet_rejected",
    "request rejected",
    "user reject",
    "approval denied",
  ];
  const lower = raw.toLowerCase();
  return needles.some((n) => lower.includes(n));
}

function isInsufficientFunds(raw: string, logs: string[]): boolean {
  const hay = `${raw}\n${logs.join("\n")}`.toLowerCase();
  return (
    hay.includes("insufficient funds for fee") ||
    hay.includes("insufficient funds for rent") ||
    hay.includes("insufficient lamports") ||
    hay.includes("attempt to debit an account but found no record of a prior credit")
  );
}

function isBlockhashExpired(raw: string, logs: string[]): boolean {
  const hay = `${raw}\n${logs.join("\n")}`.toLowerCase();
  return (
    hay.includes("blockhash not found") ||
    hay.includes("block height exceeded") ||
    hay.includes("transaction was not confirmed in") ||
    hay.includes("transaction simulation failed: blockhash")
  );
}

function isNetworkError(raw: string): boolean {
  const lower = raw.toLowerCase();
  return (
    lower.includes("failed to fetch") ||
    lower.includes("network error") ||
    lower.includes("networkerror") ||
    lower.includes("econnrefused") ||
    (lower.includes("rpc") && lower.includes("timeout"))
  );
}

// ─── Anchor custom error extraction ─────────────────────────────────

type RawAnchorError = AnchorErrorEntry & { code: number };

function extractAnchorError(err: unknown, raw: string, logs: string[]): RawAnchorError | null {
  const programFromLogs = extractProgramFromLogs(logs);

  // 1. AnchorError log lines — the richest source. Anchor 0.30+ emits
  //    variants like:
  //      "Program log: AnchorError occurred. Error Code: ProductNotRegistered. Error Number: 6040. Error Message: product not registered."
  //      "Program log: AnchorError caused by account: vault_state. Error Code: CapacityExceeded. Error Number: 6021. Error Message: vault capacity exceeded."
  //      "Program log: AnchorError thrown in programs/.../handler.rs:91. Error Code: SlippageExceeded. Error Number: 6058. Error Message: slippage bound exceeded."
  //    The shape that matters is `Error Code: X. Error Number: N. Error Message: ...`.
  for (const line of logs) {
    const m =
      /Error Code:\s*(\w+)\.\s*Error Number:\s*(\d+)\.\s*Error Message:\s*(.+?)\.?$/i.exec(line);
    if (m) {
      const code = Number(m[2]);
      return {
        code,
        name: m[1],
        msg: m[3],
        program: programFromLogs ?? lookupProgramByCode(code) ?? "program",
      };
    }
  }

  // 2. Parsed `TransactionError::InstructionError(_, Custom(N))` in logs or
  //    message: look for "custom program error: 0xHHHH" and for Custom(N).
  const hexMatch = /custom program error:\s*0x([0-9a-f]+)/i.exec(raw) ?? null;
  if (hexMatch) {
    const code = parseInt(hexMatch[1], 16);
    const fromTable =
      (programFromLogs && ERROR_BY_PROGRAM.get(programFromLogs)?.get(code)) ??
      ERROR_ANY.get(code);
    if (fromTable) return { code, ...fromTable };
    return { code, name: `Unknown code 0x${hexMatch[1]}`, msg: raw, program: programFromLogs ?? "program" };
  }
  const customMatch = /InstructionError":\[\d+,\{"Custom":(\d+)}/i.exec(raw) ?? /Custom\((\d+)\)/.exec(raw);
  if (customMatch) {
    const code = Number(customMatch[1]);
    const fromTable =
      (programFromLogs && ERROR_BY_PROGRAM.get(programFromLogs)?.get(code)) ??
      ERROR_ANY.get(code);
    if (fromTable) return { code, ...fromTable };
    return { code, name: `Unknown code ${code}`, msg: raw, program: programFromLogs ?? "program" };
  }

  return null;
}

function extractProgramFromLogs(logs: string[]): string | null {
  // The "consumed X of Y compute units" line ends every program invocation.
  // "Program <pubkey> consumed ..." — but we want the program *name* the
  // user thinks in, so fall back to whichever IDL we recognise.
  for (const log of logs) {
    const m = /Program ([1-9A-HJ-NP-Za-km-z]{32,44}) consumed/.exec(log);
    if (m) return programNameForPubkey(m[1]);
  }
  return null;
}

function programNameForPubkey(pubkey: string): string | null {
  // Look up by address in the IDLs. Each IDL's metadata.address is the
  // deployed program id (if pinned at build time).
  for (const idl of IDLS) {
    const addr = (idl as { address?: string }).address;
    if (addr && addr === pubkey) {
      return idl.metadata?.name ?? null;
    }
  }
  return null;
}

function lookupProgramByCode(code: number): string | null {
  for (const [program, table] of ERROR_BY_PROGRAM.entries()) {
    if (table.has(code)) return program;
  }
  return null;
}

// ─── Friendly translations for known error names ───────────────────

// Map from Anchor error `name` to buyer-friendly `{title, body}`. Anything
// not in this map falls back to the IDL `msg`, capitalised, as the body.
const FRIENDLY: Record<string, Partial<Pick<MappedError, "title" | "body" | "retryable">>> = {
  // Freshness gates
  SigmaStale: {
    title: "Volatility data is stale.",
    body: "The volatility feed hasn't refreshed recently. Try again in a few seconds, or check that the EWMA keeper is running.",
    retryable: true,
  },
  RegimeStale: {
    title: "Market-regime signal is stale.",
    body: "The regime-keeper hasn't written a fresh update. This usually resolves within a minute.",
    retryable: true,
  },
  RegressionStale: {
    title: "Regression inputs are stale.",
    body: "The keeper that publishes the IWM → SPY/QQQ regression hasn't written recently. Flagship quotes will resume once it refreshes (usually a few minutes).",
    retryable: true,
  },
  PythStale: {
    title: "Oracle price is stale.",
    body: "The Pyth price feed powering this quote hasn't updated recently. Wait a moment and try again.",
    retryable: true,
  },
  PythPublishTimeStale: {
    title: "Oracle price is stale.",
    body: "The Pyth publish-time on one of the underlyings is older than the cluster's staleness cap. Wait for a fresh price.",
    retryable: true,
  },
  PythPublishTimeFuture: {
    title: "Oracle price is from the future.",
    body: "The Pyth feed reports a publish-time newer than the cluster's clock. This is a safety gate against forged feeds.",
    retryable: true,
  },
  PythPublishTimeNotMonotonic: {
    title: "Oracle price went backwards.",
    body: "A Pyth publish-time is older than the previously recorded one. The kernel rejects replayed feeds.",
    retryable: true,
  },

  // Pause flags
  PausedGlobally: {
    title: "Issuance is paused protocol-wide.",
    body: "An operator has paused all new issuance. Check the protocol announcements.",
    retryable: false,
  },
  IssuancePausedPerProduct: {
    title: "This product is paused.",
    body: "New issuance is paused for this product. Try a different product or check back later.",
    retryable: false,
  },
  ProductPaused: {
    title: "This product is paused.",
    body: "An operator has paused this product. New positions can't be opened right now.",
    retryable: false,
  },
  ProductNotRegistered: {
    title: "This product isn't registered on the current cluster.",
    body: "The network you're connected to doesn't have this product deployed. Switch cluster in Network settings.",
    retryable: true,
  },

  // Capacity / caps
  CapacityExceeded: {
    title: "Vault capacity is full.",
    body: "The underwriting vault can't take on more risk right now. Try a smaller notional or wait for existing positions to expire.",
    retryable: true,
  },
  UtilizationCapExceeded: {
    title: "Protocol utilisation is too high.",
    body: "The protocol has hit its utilisation cap. Try a smaller notional or wait until positions unwind.",
    retryable: true,
  },
  RiskCapExceeded: {
    title: "Above the per-note risk cap.",
    body: "This position would exceed the per-note risk limit. Reduce the notional and try again.",
    retryable: true,
  },
  GlobalRiskCapExceeded: {
    title: "Above the per-product risk cap.",
    body: "This product has hit its aggregate risk cap. Try a smaller notional or wait for positions to expire.",
    retryable: true,
  },

  // Slippage / quote freshness
  SlippageExceeded: {
    title: "Price moved beyond your slippage tolerance.",
    body: "The on-chain quote shifted more than you allowed. Refresh the quote or widen slippage in Advanced safeguards.",
    retryable: true,
  },
  BelowMinimumTrade: {
    title: "Below the minimum ticket.",
    body: "Increase the notional — the minimum is usually $100.",
    retryable: true,
  },
  InvalidQuoteExpiry: {
    title: "Quote has expired.",
    body: "Refresh the quote — the one you had has timed out.",
    retryable: true,
  },

  // Lifecycle
  PolicyNotQuoted: {
    title: "No active quote.",
    body: "Preview a quote before trying to buy.",
    retryable: true,
  },
  PolicyNotActive: {
    title: "This position isn't active.",
    body: "The position isn't in a state that allows this action.",
    retryable: false,
  },

  // Authentication
  KeeperAuthorityMismatch: {
    title: "Keeper signature rejected.",
    body: "An off-chain keeper tried to write state with the wrong authority. This shouldn't affect you as a buyer.",
    retryable: false,
  },
  AdminMismatch: {
    title: "Admin signature rejected.",
    body: "An admin action was attempted with a key that isn't the registered admin.",
    retryable: false,
  },

  // Flagship-specific
  CorrectionTableHashMismatch: {
    title: "Pricing correction table hash mismatch.",
    body: "The frontend's expected pricing correction table doesn't match what the program expects. Refresh the page.",
    retryable: true,
  },

  // Overflow / generic numeric
  Overflow: {
    title: "Arithmetic overflow.",
    body: "The numbers in this quote overflowed the kernel's fixed-point range. Try a smaller notional.",
    retryable: true,
  },
};

function translateAnchorError(err: RawAnchorError): MappedError {
  const friendly = FRIENDLY[err.name];
  if (friendly) {
    return {
      title: friendly.title ?? capitalise(err.msg),
      body: friendly.body ?? capitalise(err.msg),
      retryable: friendly.retryable ?? true,
      detail: `${err.program}: ${err.name} (code ${err.code})`,
    };
  }
  return {
    title: humaniseErrorName(err.name),
    body: capitalise(err.msg),
    retryable: true,
    detail: `${err.program}: ${err.name} (code ${err.code})`,
  };
}

function humaniseErrorName(name: string): string {
  // "ProductNotRegistered" → "Product not registered."
  const spaced = name.replace(/([A-Z])/g, " $1").trim();
  return capitalise(spaced).replace(/\.?$/, ".");
}

function capitalise(msg: string): string {
  if (!msg) return "";
  return msg.charAt(0).toUpperCase() + msg.slice(1);
}
