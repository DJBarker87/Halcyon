/**
 * Print the deterministic PriceUpdateV2 account addresses for the feeds
 * in the relay config, without starting the relay itself. Handy for
 * populating `frontend/.env.local` + keeper configs before the relay's
 * first successful post lands.
 *
 * Uses the SDK's standalone PDA helper so we can skip Connection + Wallet
 * instantiation entirely — avoids the `@solana/web3.js` ↔ `rpc-websockets`
 * ESM resolution bug on Node 22+.
 *
 * Usage:
 *   npm run addresses                         # uses ./config/devnet.json
 *   npm run addresses -- --config other.json
 */

import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";
import { PublicKey } from "@solana/web3.js";

type FeedConfig = { alias: string; id: string };

const { values } = parseArgs({
  options: {
    config: { type: "string", default: "./config/devnet.json" },
  },
});

const raw = readFileSync(values.config as string, "utf8");
const cfg = JSON.parse(raw) as {
  shard_id: number;
  feeds: FeedConfig[];
};

// The Pyth-blessed push-oracle program ID — same on devnet and mainnet.
// Source: `@pythnetwork/pyth-solana-receiver` SDK. Deterministic PDAs
// for the push-oracle accounts Halcyon reads are derived as
// `PublicKey.findProgramAddressSync([shardIdLE_u16, feedIdBytes], this)`.
const PYTH_PUSH_ORACLE_ID = new PublicKey(
  "pythWSnswVUd12oZpeFP8e9CVaEqJg25g1Vtc2biRsT",
);

function getPriceFeedAccountAddress(shardId: number, feedIdHex: string): PublicKey {
  const hex = feedIdHex.startsWith("0x") ? feedIdHex.slice(2) : feedIdHex;
  const feedIdBytes = Buffer.from(hex, "hex");
  if (feedIdBytes.length !== 32) {
    throw new Error(`feed id must be 32 bytes, got ${feedIdBytes.length}`);
  }
  const shardBuf = Buffer.alloc(2);
  shardBuf.writeUInt16LE(shardId, 0);
  const [pda] = PublicKey.findProgramAddressSync(
    [shardBuf, feedIdBytes],
    PYTH_PUSH_ORACLE_ID,
  );
  return pda;
}

console.log(`shard_id      = ${cfg.shard_id}`);
console.log(`push_oracle   = ${PYTH_PUSH_ORACLE_ID.toBase58()}\n`);

for (const feed of cfg.feeds) {
  const address = getPriceFeedAccountAddress(cfg.shard_id, feed.id);
  const symbol = feed.alias.replace("_USD", "").toUpperCase();
  const envKey = `NEXT_PUBLIC_PYTH_${symbol}_ACCOUNT_DEVNET`;
  console.log(`${feed.alias}`);
  console.log(`  account : ${address.toBase58()}`);
  console.log(`  env     : ${envKey}=${address.toBase58()}\n`);
}
