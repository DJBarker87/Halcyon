import { createReadStream, createWriteStream, existsSync } from "node:fs";
import { appendFile, mkdir, open, readFile, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { finished } from "node:stream/promises";

export type LogLevel = "INFO" | "WARN" | "ERROR";
export type Logger = (level: LogLevel, msg: string, fields?: Record<string, unknown>) => void;

export type FeedConfig = {
  alias: string;
  id: string;
};

export type RelayCacheConfig = {
  cache_dir: string;
  cache_retention_days: number;
  cache_feeds: string[];
  feeds: FeedConfig[];
};

export type CachedPricePoint = {
  feed_id: string;
  feed_alias: string;
  publish_time: number;
  price: string;
  conf: string;
  exponent: string;
  account: string;
};

const CACHE_SCHEMA_HEADER = "publish_time,price,conf,exponent";
const PRUNE_MARKER_FILE = ".prune-state.json";
const PRUNE_INTERVAL_MS = 7 * 24 * 60 * 60 * 1000;
const FIRST_LINE_READ_BYTES = 512;
const LAST_LINE_READ_BYTES = 8192;

export function defaultCacheDir(): string {
  return "/var/lib/halcyon/relay-cache";
}

export function defaultCacheRetentionDays(): number {
  return 450;
}

export function defaultCacheFeeds(feeds: FeedConfig[]): string[] {
  return feeds.map((feed) => feed.alias);
}

export function normaliseCacheFeeds(cacheFeeds: string[] | undefined, feeds: FeedConfig[]): string[] {
  const requested = (cacheFeeds ?? defaultCacheFeeds(feeds))
    .map((feed) => feed.trim())
    .filter((feed) => feed.length > 0);
  return [...new Set(requested)];
}

export function validateCacheFeeds(cacheFeeds: string[], feeds: FeedConfig[]): void {
  const feedAliases = new Set(feeds.map((feed) => feed.alias));
  const unknown = cacheFeeds.filter((feed) => !feedAliases.has(feed));
  if (unknown.length > 0) {
    throw new Error(`unknown cache_feeds aliases: ${unknown.join(", ")}`);
  }
}

export async function ensureRelayCacheDir(cacheDir: string): Promise<void> {
  await mkdir(cacheDir, { recursive: true });
}

export async function maybePruneRelayCache(
  cfg: RelayCacheConfig,
  log: Logger,
  force = false,
): Promise<void> {
  try {
    await ensureRelayCacheDir(cfg.cache_dir);
    if (!force && !(await pruneDue(cfg.cache_dir))) {
      return;
    }

    const cutoffPublishTime = Math.floor(Date.now() / 1000) - cfg.cache_retention_days * 24 * 60 * 60;
    for (const alias of cfg.cache_feeds) {
      await pruneFeedCacheFile(cfg.cache_dir, alias, cutoffPublishTime, log);
    }
    await writePruneMarker(cfg.cache_dir);
    log("INFO", "relay cache prune complete", {
      cache_dir: cfg.cache_dir,
      cache_retention_days: cfg.cache_retention_days,
      cache_feed_count: cfg.cache_feeds.length,
      forced: force,
    });
  } catch (error) {
    log("WARN", "relay cache prune skipped", {
      cache_dir: cfg.cache_dir,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

export async function appendFeedCacheEntry(
  cfg: RelayCacheConfig,
  row: CachedPricePoint,
  log: Logger,
): Promise<void> {
  if (!cfg.cache_feeds.includes(row.feed_alias)) {
    return;
  }

  await ensureRelayCacheDir(cfg.cache_dir);
  const cachePath = feedCachePath(cfg.cache_dir, row.feed_alias);
  const schemaOk = await ensureFeedCacheSchema(cachePath, row.feed_alias, log);
  if (!schemaOk) {
    return;
  }

  const lastDataLine = await readLastDataLine(cachePath);
  if (lastDataLine !== null) {
    const parsed = parseCacheLine(lastDataLine);
    if (parsed === null) {
      log("WARN", "relay cache tail unreadable; skipping append", {
        alias: row.feed_alias,
        cache_path: cachePath,
      });
      return;
    }
    if (parsed.publish_time === row.publish_time) {
      log("INFO", "relay cache duplicate skipped", {
        alias: row.feed_alias,
        cache_path: cachePath,
        publish_time: row.publish_time,
      });
      return;
    }
    if (parsed.publish_time > row.publish_time) {
      log("WARN", "relay cache out-of-order publish_time skipped", {
        alias: row.feed_alias,
        cache_path: cachePath,
        cached_publish_time: parsed.publish_time,
        attempted_publish_time: row.publish_time,
      });
      return;
    }
  }

  await appendFile(
    cachePath,
    `${row.publish_time},${row.price},${row.conf},${row.exponent}\n`,
    "utf8",
  );
  log("INFO", "relay cache appended", {
    alias: row.feed_alias,
    account: row.account,
    cache_path: cachePath,
    publish_time: row.publish_time,
  });
}

function feedCachePath(cacheDir: string, alias: string): string {
  return join(cacheDir, `${alias}.csv`);
}

async function ensureFeedCacheSchema(cachePath: string, alias: string, log: Logger): Promise<boolean> {
  if (!existsSync(cachePath)) {
    await writeFile(cachePath, `${CACHE_SCHEMA_HEADER}\n`, "utf8");
    return true;
  }

  const firstLine = await readFirstLine(cachePath);
  if (firstLine === CACHE_SCHEMA_HEADER) {
    return true;
  }

  log("WARN", "relay cache schema mismatch; skipping feed cache writes", {
    alias,
    cache_path: cachePath,
    expected_schema: CACHE_SCHEMA_HEADER,
    found_schema: firstLine ?? "",
  });
  return false;
}

async function readFirstLine(cachePath: string): Promise<string | null> {
  const handle = await open(cachePath, "r");
  try {
    const { size } = await handle.stat();
    if (size === 0) {
      return null;
    }
    const length = Math.min(Number(size), FIRST_LINE_READ_BYTES);
    const buffer = Buffer.alloc(length);
    const { bytesRead } = await handle.read(buffer, 0, length, 0);
    return buffer
      .subarray(0, bytesRead)
      .toString("utf8")
      .split(/\r?\n/, 1)[0] ?? null;
  } finally {
    await handle.close();
  }
}

async function readLastDataLine(cachePath: string): Promise<string | null> {
  const handle = await open(cachePath, "r");
  try {
    const { size } = await handle.stat();
    if (size === 0) {
      return null;
    }
    const length = Math.min(Number(size), LAST_LINE_READ_BYTES);
    const buffer = Buffer.alloc(length);
    const { bytesRead } = await handle.read(buffer, 0, length, Number(size) - length);
    const lines = buffer
      .subarray(0, bytesRead)
      .toString("utf8")
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    for (let idx = lines.length - 1; idx >= 0; idx -= 1) {
      if (lines[idx] !== CACHE_SCHEMA_HEADER) {
        return lines[idx];
      }
    }
    return null;
  } finally {
    await handle.close();
  }
}

async function pruneDue(cacheDir: string): Promise<boolean> {
  const markerPath = join(cacheDir, PRUNE_MARKER_FILE);
  if (!existsSync(markerPath)) {
    return true;
  }
  try {
    const payload = JSON.parse(await readFile(markerPath, "utf8")) as { last_pruned_ms?: number };
    if (typeof payload.last_pruned_ms !== "number") {
      return true;
    }
    return Date.now() - payload.last_pruned_ms >= PRUNE_INTERVAL_MS;
  } catch {
    return true;
  }
}

async function writePruneMarker(cacheDir: string): Promise<void> {
  const markerPath = join(cacheDir, PRUNE_MARKER_FILE);
  await writeFile(
    markerPath,
    JSON.stringify({ last_pruned_ms: Date.now() }),
    "utf8",
  );
}

async function pruneFeedCacheFile(
  cacheDir: string,
  alias: string,
  cutoffPublishTime: number,
  log: Logger,
): Promise<void> {
  const cachePath = feedCachePath(cacheDir, alias);
  if (!existsSync(cachePath)) {
    return;
  }

  const firstLine = await readFirstLine(cachePath);
  if (firstLine !== CACHE_SCHEMA_HEADER) {
    log("WARN", "relay cache prune skipped due to schema mismatch", {
      alias,
      cache_path: cachePath,
      expected_schema: CACHE_SCHEMA_HEADER,
      found_schema: firstLine ?? "",
    });
    return;
  }

  const tempPath = `${cachePath}.tmp.${process.pid}`;
  const writer = createWriteStream(tempPath, { encoding: "utf8" });
  writer.write(`${CACHE_SCHEMA_HEADER}\n`);
  let keptRows = 0;
  let prunedRows = 0;
  let malformedRows = 0;

  try {
    const reader = createInterface({
      input: createReadStream(cachePath, { encoding: "utf8" }),
      crlfDelay: Infinity,
    });
    let isHeader = true;
    for await (const line of reader) {
      if (isHeader) {
        isHeader = false;
        continue;
      }
      const trimmed = line.trim();
      if (trimmed.length === 0) {
        continue;
      }
      const parsed = parseCacheLine(trimmed);
      if (parsed === null) {
        malformedRows += 1;
        continue;
      }
      if (parsed.publish_time >= cutoffPublishTime) {
        writer.write(`${trimmed}\n`);
        keptRows += 1;
      } else {
        prunedRows += 1;
      }
    }
    writer.end();
    await finished(writer);
    await rename(tempPath, cachePath);
    log("INFO", "relay cache pruned", {
      alias,
      cache_path: cachePath,
      cutoff_publish_time: cutoffPublishTime,
      kept_rows: keptRows,
      pruned_rows: prunedRows,
      malformed_rows: malformedRows,
    });
  } catch (error) {
    writer.destroy();
    throw error;
  } finally {
    await rm(tempPath, { force: true });
  }
}

function parseCacheLine(line: string): { publish_time: number } | null {
  const parts = line.split(",");
  if (parts.length !== 4) {
    return null;
  }
  const publishTime = Number(parts[0]);
  if (!Number.isFinite(publishTime)) {
    return null;
  }
  return { publish_time: publishTime };
}

export function cacheSchemaHeaderForTests(): string {
  return CACHE_SCHEMA_HEADER;
}
