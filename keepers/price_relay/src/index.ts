/**
 * Halcyon price relay.
 *
 * Pulls Pyth VAAs from Hermes and posts them to `PriceUpdateV2` accounts
 * on the target Solana cluster (devnet or mainnet). Halcyon programs
 * then read those accounts via `halcyon_oracles::pyth::read_pyth_price`
 * with `VerificationLevel::Full`.
 *
 * Why this exists:
 *   Pyth publishes SPY/QQQ/IWM feeds to mainnet's `pyth-solana-receiver`
 *   but not to devnet. Halcyon's flagship pricer needs those feeds on
 *   whichever cluster we're running against. This relay uses
 *   `@pythnetwork/pyth-solana-receiver` (the Pyth-blessed SDK) to do
 *   the full Wormhole + receiver dance and land prices in accounts we
 *   control.
 *
 * Deterministic addresses:
 *   The SDK derives `PriceUpdateV2` account addresses as
 *   `findProgramAddress([shardLE_u16, feedIdBytes_32], receiver)`.
 *   With a fixed `shard_id` in config, the 5 feed addresses are stable
 *   across restarts and can be pinned into frontend `.env.local` and
 *   keeper configs. Print them via `npm run addresses`.
 */

import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";
import {
  Connection,
  Keypair,
} from "@solana/web3.js";
import { Wallet } from "@coral-xyz/anchor";
import { HermesClient } from "@pythnetwork/hermes-client";
import { PythSolanaReceiver } from "@pythnetwork/pyth-solana-receiver";
import {
  appendFeedCacheEntry,
  defaultCacheDir,
  defaultCacheRetentionDays,
  maybePruneRelayCache,
  normaliseCacheFeeds,
  validateCacheFeeds,
  type FeedConfig,
  type RelayCacheConfig,
} from "./cache";

// ---------- Config ----------

type RelayConfig = {
  rpc_endpoint: string;
  hermes_endpoint: string;
  keypair_path: string;
  shard_id: number;
  scan_interval_secs: number;
  staleness_cap_secs: number;
  feeds: FeedConfig[];
  failure_budget: number;
  backoff_cap_secs: number;
  cache_dir: string;
  cache_retention_days: number;
  cache_feeds: string[];
};

function loadConfig(path: string): RelayConfig {
  const raw = readFileSync(path, "utf8");
  const cfg = JSON.parse(raw) as Partial<RelayConfig>;
  // Minimal validation — bail loudly on missing essentials.
  const required: (keyof RelayConfig)[] = [
    "rpc_endpoint",
    "hermes_endpoint",
    "keypair_path",
    "shard_id",
    "scan_interval_secs",
    "feeds",
  ];
  for (const key of required) {
    if (cfg[key] === undefined || cfg[key] === null) {
      throw new Error(`config at ${path} is missing required field: ${String(key)}`);
    }
  }
  const feeds = cfg.feeds!;
  const cacheFeeds = normaliseCacheFeeds(cfg.cache_feeds, feeds);
  validateCacheFeeds(cacheFeeds, feeds);
  const cacheRetentionDays = cfg.cache_retention_days ?? defaultCacheRetentionDays();
  if (!Number.isInteger(cacheRetentionDays) || cacheRetentionDays < 0) {
    throw new Error(`config at ${path} has invalid cache_retention_days=${String(cfg.cache_retention_days)}`);
  }
  return {
    rpc_endpoint: cfg.rpc_endpoint!,
    hermes_endpoint: cfg.hermes_endpoint!,
    keypair_path: cfg.keypair_path!,
    shard_id: cfg.shard_id!,
    scan_interval_secs: cfg.scan_interval_secs!,
    staleness_cap_secs: cfg.staleness_cap_secs ?? 30,
    feeds,
    failure_budget: cfg.failure_budget ?? 5,
    backoff_cap_secs: cfg.backoff_cap_secs ?? 60,
    cache_dir: cfg.cache_dir ?? defaultCacheDir(),
    cache_retention_days: cacheRetentionDays,
    cache_feeds: cacheFeeds,
  };
}

function loadKeypair(path: string): Keypair {
  const bytes = JSON.parse(readFileSync(path, "utf8")) as number[];
  return Keypair.fromSecretKey(Uint8Array.from(bytes));
}

// ---------- Logging (JSON, journald-friendly) ----------

type LogLevel = "INFO" | "WARN" | "ERROR";

function redactSecrets(value: unknown): unknown {
  if (typeof value === "string") {
    return value
      .replace(/([?&](?:api-?key|token)=)[^&\s"]+/gi, "$1<redacted>")
      .replace(/(HALCYON_API_KEY=)[^\s"]+/g, "$1<redacted>");
  }
  if (Array.isArray(value)) {
    return value.map((item) => redactSecrets(item));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, redactSecrets(item)]),
    );
  }
  return value;
}

function log(level: LogLevel, msg: string, fields: Record<string, unknown> = {}) {
  const line = JSON.stringify({
    ts: new Date().toISOString(),
    level,
    target: "halcyon_price_relay",
    msg,
    ...(redactSecrets(fields) as Record<string, unknown>),
  });
  if (level === "ERROR") {
    process.stderr.write(line + "\n");
  } else {
    process.stdout.write(line + "\n");
  }
}

// ---------- Main loop ----------

function cacheConfigFromRelayConfig(cfg: RelayConfig): RelayCacheConfig {
  return {
    cache_dir: cfg.cache_dir,
    cache_retention_days: cfg.cache_retention_days,
    cache_feeds: cfg.cache_feeds,
    feeds: cfg.feeds,
  };
}

function integerString(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new Error(`non-finite numeric field: ${String(value)}`);
    }
    return Math.trunc(value).toString();
  }
  if (typeof value === "bigint") {
    return value.toString();
  }
  if (value && typeof value === "object" && "toString" in value && typeof value.toString === "function") {
    const rendered = value.toString();
    if (rendered.length > 0 && rendered !== "[object Object]") {
      return rendered;
    }
  }
  throw new Error(`unsupported integer field type: ${String(value)}`);
}

async function cachePostedFeeds(cfg: RelayConfig, receiver: PythSolanaReceiver): Promise<void> {
  const cacheCfg = cacheConfigFromRelayConfig(cfg);
  for (const feed of cfg.feeds) {
    if (!cfg.cache_feeds.includes(feed.alias)) {
      continue;
    }
    const account = receiver.getPriceFeedAccountAddress(cfg.shard_id, feed.id);
    try {
      const fetched = await receiver.fetchPriceFeedAccount(cfg.shard_id, feed.id);
      if (fetched === null) {
        log("WARN", "relay cache fetch returned null", {
          alias: feed.alias,
          account: account.toBase58(),
        });
        continue;
      }
      const priceMessage = fetched.priceMessage;
      const publishTime = Number(integerString(priceMessage.publishTime));
      if (!Number.isFinite(publishTime)) {
        throw new Error(`invalid publish_time ${String(priceMessage.publishTime)}`);
      }
      await appendFeedCacheEntry(
        cacheCfg,
        {
          feed_id: feed.id,
          feed_alias: feed.alias,
          publish_time: publishTime,
          price: integerString(priceMessage.price),
          conf: integerString(priceMessage.conf),
          exponent: integerString(priceMessage.exponent),
          account: account.toBase58(),
        },
        log,
      );
    } catch (error) {
      log("WARN", "relay cache fetch skipped", {
        alias: feed.alias,
        account: account.toBase58(),
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }
}

async function runOnce(
  cfg: RelayConfig,
  receiver: PythSolanaReceiver,
  hermes: HermesClient,
): Promise<void> {
  const priceFeedIds = cfg.feeds.map((f) => f.id);
  const priceUpdates = await hermes.getLatestPriceUpdates(priceFeedIds, {
    encoding: "base64",
  });
  const vaas = priceUpdates.binary.data;
  if (!Array.isArray(vaas) || vaas.length === 0) {
    throw new Error("hermes returned no price-update payloads");
  }

  // `addUpdatePriceFeed` is the method that writes to the push-oracle's
  // deterministic PDAs (seed = [shard_le_u16, feed_id_32]). The sibling
  // `addPostPriceUpdates` writes to EPHEMERAL accounts — correct for
  // one-shot consumers but wrong for a relay whose whole point is to
  // land prices at stable, pre-publishable addresses.
  const txBuilder = receiver.newTransactionBuilder({
    closeUpdateAccounts: false, // keep encoded-VAA accounts around across cycles
  });
  await txBuilder.addUpdatePriceFeed(vaas, cfg.shard_id);

  const versionedTxs = await txBuilder.buildVersionedTransactions({
    computeUnitPriceMicroLamports: 50_000,
  });
  if (versionedTxs.length === 0) {
    log("INFO", "no on-chain updates needed this cycle");
    return;
  }

  const signatures: string[] = [];
  for (const ix of versionedTxs) {
    const sig = await receiver.provider.sendAndConfirm!(ix.tx, ix.signers);
    signatures.push(sig);
  }

  for (const feed of cfg.feeds) {
    const address = receiver.getPriceFeedAccountAddress(cfg.shard_id, feed.id);
    log("INFO", "posted", {
      alias: feed.alias,
      account: address.toBase58(),
    });
  }
  await cachePostedFeeds(cfg, receiver);
  log("INFO", "cycle complete", { txs: signatures.length });
}

async function runForever(cfg: RelayConfig): Promise<void> {
  const connection = new Connection(cfg.rpc_endpoint, "confirmed");
  const keypair = loadKeypair(cfg.keypair_path);
  const wallet = new Wallet(keypair);
  const hermes = new HermesClient(cfg.hermes_endpoint, {});
  const receiver = new PythSolanaReceiver({
    connection,
    wallet,
  });

  log("INFO", "price relay starting", {
    endpoint: cfg.rpc_endpoint,
    wallet: keypair.publicKey.toBase58(),
    shard_id: cfg.shard_id,
    feed_count: cfg.feeds.length,
    scan_interval_secs: cfg.scan_interval_secs,
    cache_dir: cfg.cache_dir,
    cache_retention_days: cfg.cache_retention_days,
    cache_feeds: cfg.cache_feeds,
  });

  // Pre-compute and log the 5 deterministic PriceUpdateV2 addresses so
  // operators can pin them into frontend .env.local and keeper configs
  // before the first post lands.
  for (const feed of cfg.feeds) {
    const address = receiver.getPriceFeedAccountAddress(cfg.shard_id, feed.id);
    log("INFO", "feed address", {
      alias: feed.alias,
      feed_id: feed.id,
      account: address.toBase58(),
    });
  }

  let consecutiveFailures = 0;
  let backoffSecs = 1;

  const shutdown = new Promise<void>((resolve) => {
    const onSignal = (name: string) => {
      log("INFO", "signal received, shutting down", { signal: name });
      resolve();
    };
    process.on("SIGINT", () => onSignal("SIGINT"));
    process.on("SIGTERM", () => onSignal("SIGTERM"));
  });

  let stopped = false;
  shutdown.then(() => {
    stopped = true;
  });

  await maybePruneRelayCache(cacheConfigFromRelayConfig(cfg), log, true);

  while (!stopped) {
    try {
      await maybePruneRelayCache(cacheConfigFromRelayConfig(cfg), log);
      await runOnce(cfg, receiver, hermes);
      consecutiveFailures = 0;
      backoffSecs = 1;
      await sleep(cfg.scan_interval_secs * 1000);
    } catch (err) {
      consecutiveFailures += 1;
      const message = err instanceof Error ? err.message : String(err);
      log("ERROR", "cycle failed", {
        consecutive_failures: consecutiveFailures,
        error: message,
      });
      if (consecutiveFailures >= cfg.failure_budget) {
        log("ERROR", "failure budget exhausted, exiting for ops alert", {
          failure_budget: cfg.failure_budget,
        });
        process.exitCode = 1;
        return;
      }
      await sleep(backoffSecs * 1000);
      backoffSecs = Math.min(backoffSecs * 2, cfg.backoff_cap_secs);
    }
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ---------- CLI ----------

async function main() {
  const { values } = parseArgs({
    options: {
      config: { type: "string", default: "./config/devnet.json" },
      once: { type: "boolean", default: false },
    },
  });
  const cfg = loadConfig(values.config as string);

  if (values.once) {
    const connection = new Connection(cfg.rpc_endpoint, "confirmed");
    const wallet = new Wallet(loadKeypair(cfg.keypair_path));
    const hermes = new HermesClient(cfg.hermes_endpoint, {});
    const receiver = new PythSolanaReceiver({
      connection,
      wallet,
    });
    await maybePruneRelayCache(cacheConfigFromRelayConfig(cfg), log, true);
    await runOnce(cfg, receiver, hermes);
    return;
  }

  await runForever(cfg);
}

main().catch((err) => {
  log("ERROR", "fatal", { error: err instanceof Error ? err.stack : String(err) });
  process.exit(1);
});
