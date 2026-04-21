import { createHash } from "crypto";
import fs from "fs";
import path from "path";

import { PublicKey } from "@solana/web3.js";

export const PROGRAM_IDS = {
  kernel: new PublicKey("H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF"),
  solAutocall: new PublicKey("6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8"),
  ilProtection: new PublicKey("HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g"),
  flagshipAutocall: new PublicKey("E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc"),
} as const;

export const FEED_IDS = {
  solUsd:
    "ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d",
  usdcUsd:
    "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a",
  spyUsd:
    "19e09bb805456ada3979a7d1cbb4b6d63babc3a0f8e8a9509f68afa5c4c11cd5",
  qqqUsd:
    "9695e2b96ea7b3859da9ed25b7a46a920a776e2fdae19a7bcfdf2b219230452d",
  iwmUsd:
    "eff690a187797aa225723345d4612abec0bf0cec1ae62347c0e7b1905d730879",
} as const;

const MOCK_DISCRIMINATOR = Buffer.from("HMOCKPYT", "ascii");
const DEFAULT_CONF_S6 = 10_000;
const DEFAULT_LAMPORTS = 1_000_000;
const DEFAULT_RENT_EPOCH = 0;

type ProgramKey = keyof typeof PROGRAM_IDS;
type FeedKey = keyof typeof FEED_IDS;

export type MockOracleFixture = {
  file: string;
  feedIdHex: string;
  label: string;
  owner: string;
  priceS6: number;
  pubkey: string;
  publishSlot: number;
  publishTs: number;
};

export type MockOracleManifest = {
  accountCount: number;
  baseTimestamp: number;
  fixtures: Record<string, MockOracleFixture>;
};

type FixtureSpec = {
  feed: FeedKey;
  label: string;
  owner: ProgramKey;
  priceS6: number;
  publishSlot: number;
  publishTs: number;
};

function deterministicPubkey(label: string): PublicKey {
  const digest = createHash("sha256")
    .update(`halcyon-mock-pyth:${label}`)
    .digest();
  return new PublicKey(digest.subarray(0, 32));
}

function feedIdBuffer(feed: FeedKey): Buffer {
  return Buffer.from(FEED_IDS[feed], "hex");
}

function encodeMockPriceAccount(spec: FixtureSpec): Buffer {
  const out = Buffer.alloc(8 + 32 + 8 + 8 + 4 + 8 + 8);
  let offset = 0;

  MOCK_DISCRIMINATOR.copy(out, offset);
  offset += 8;

  feedIdBuffer(spec.feed).copy(out, offset);
  offset += 32;

  out.writeBigInt64LE(BigInt(spec.priceS6), offset);
  offset += 8;

  out.writeBigInt64LE(BigInt(DEFAULT_CONF_S6), offset);
  offset += 8;

  out.writeInt32LE(-6, offset);
  offset += 4;

  out.writeBigInt64LE(BigInt(spec.publishTs), offset);
  offset += 8;

  out.writeBigUInt64LE(BigInt(spec.publishSlot), offset);
  return out;
}

function accountJson(pubkey: PublicKey, owner: PublicKey, data: Buffer): string {
  return JSON.stringify(
    {
      pubkey: pubkey.toBase58(),
      account: {
        lamports: DEFAULT_LAMPORTS,
        data: [data.toString("base64"), "base64"],
        owner: owner.toBase58(),
        executable: false,
        rentEpoch: DEFAULT_RENT_EPOCH,
      },
    },
    null,
    2
  );
}

function fixtureSpecs(baseTimestamp: number): FixtureSpec[] {
  const staleTs = baseTimestamp - 7_200;

  return [
    {
      label: "kernel-sol-init",
      owner: "kernel",
      feed: "solUsd",
      priceS6: 100_000_000,
      publishTs: baseTimestamp,
      publishSlot: 10,
    },
    {
      label: "kernel-sol-bump",
      owner: "kernel",
      feed: "solUsd",
      priceS6: 103_000_000,
      publishTs: baseTimestamp + 5,
      publishSlot: 11,
    },
    {
      label: "kernel-spy-init",
      owner: "kernel",
      feed: "spyUsd",
      priceS6: 100_000_000,
      publishTs: baseTimestamp,
      publishSlot: 20,
    },
    {
      label: "kernel-spy-bump",
      owner: "kernel",
      feed: "spyUsd",
      priceS6: 103_000_000,
      publishTs: baseTimestamp + 5,
      publishSlot: 21,
    },
    {
      label: "sol-entry",
      owner: "solAutocall",
      feed: "solUsd",
      priceS6: 100_000_000,
      publishTs: baseTimestamp + 2,
      publishSlot: 100,
    },
    {
      label: "sol-autocall",
      owner: "solAutocall",
      feed: "solUsd",
      priceS6: 103_000_000,
      publishTs: baseTimestamp + 8,
      publishSlot: 101,
    },
    {
      label: "sol-knock-in",
      owner: "solAutocall",
      feed: "solUsd",
      priceS6: 65_000_000,
      publishTs: baseTimestamp + 20,
      publishSlot: 102,
    },
    {
      label: "sol-stale",
      owner: "solAutocall",
      feed: "solUsd",
      priceS6: 100_000_000,
      publishTs: staleTs,
      publishSlot: 1,
    },
    {
      label: "il-sol-entry",
      owner: "ilProtection",
      feed: "solUsd",
      priceS6: 100_000_000,
      publishTs: baseTimestamp + 2,
      publishSlot: 200,
    },
    {
      label: "il-sol-crash",
      owner: "ilProtection",
      feed: "solUsd",
      priceS6: 50_000_000,
      publishTs: baseTimestamp + 10,
      publishSlot: 201,
    },
    {
      label: "il-sol-stale",
      owner: "ilProtection",
      feed: "solUsd",
      priceS6: 100_000_000,
      publishTs: staleTs,
      publishSlot: 2,
    },
    {
      label: "il-usdc-entry",
      owner: "ilProtection",
      feed: "usdcUsd",
      priceS6: 1_000_000,
      publishTs: baseTimestamp + 2,
      publishSlot: 202,
    },
    {
      label: "il-usdc-stale",
      owner: "ilProtection",
      feed: "usdcUsd",
      priceS6: 1_000_000,
      publishTs: staleTs,
      publishSlot: 3,
    },
    {
      label: "flagship-spy-entry",
      owner: "flagshipAutocall",
      feed: "spyUsd",
      priceS6: 100_000_000,
      publishTs: baseTimestamp + 2,
      publishSlot: 300,
    },
    {
      label: "flagship-qqq-entry",
      owner: "flagshipAutocall",
      feed: "qqqUsd",
      priceS6: 100_000_000,
      publishTs: baseTimestamp + 2,
      publishSlot: 301,
    },
    {
      label: "flagship-iwm-entry",
      owner: "flagshipAutocall",
      feed: "iwmUsd",
      priceS6: 100_000_000,
      publishTs: baseTimestamp + 2,
      publishSlot: 302,
    },
    {
      label: "flagship-spy-high",
      owner: "flagshipAutocall",
      feed: "spyUsd",
      priceS6: 103_000_000,
      publishTs: baseTimestamp + 10,
      publishSlot: 303,
    },
    {
      label: "flagship-qqq-high",
      owner: "flagshipAutocall",
      feed: "qqqUsd",
      priceS6: 103_000_000,
      publishTs: baseTimestamp + 10,
      publishSlot: 304,
    },
    {
      label: "flagship-iwm-high",
      owner: "flagshipAutocall",
      feed: "iwmUsd",
      priceS6: 103_000_000,
      publishTs: baseTimestamp + 10,
      publishSlot: 305,
    },
    {
      label: "flagship-spy-low",
      owner: "flagshipAutocall",
      feed: "spyUsd",
      priceS6: 75_000_000,
      publishTs: baseTimestamp + 20,
      publishSlot: 306,
    },
    {
      label: "flagship-qqq-low",
      owner: "flagshipAutocall",
      feed: "qqqUsd",
      priceS6: 76_000_000,
      publishTs: baseTimestamp + 20,
      publishSlot: 307,
    },
    {
      label: "flagship-iwm-low",
      owner: "flagshipAutocall",
      feed: "iwmUsd",
      priceS6: 74_000_000,
      publishTs: baseTimestamp + 20,
      publishSlot: 308,
    },
    {
      label: "flagship-spy-stale",
      owner: "flagshipAutocall",
      feed: "spyUsd",
      priceS6: 100_000_000,
      publishTs: staleTs,
      publishSlot: 4,
    },
    {
      label: "flagship-qqq-stale",
      owner: "flagshipAutocall",
      feed: "qqqUsd",
      priceS6: 100_000_000,
      publishTs: staleTs,
      publishSlot: 5,
    },
    {
      label: "flagship-iwm-stale",
      owner: "flagshipAutocall",
      feed: "iwmUsd",
      priceS6: 100_000_000,
      publishTs: staleTs,
      publishSlot: 6,
    },
  ];
}

export function buildMockOracleManifest(
  baseTimestamp: number,
  fixturesDir: string
): MockOracleManifest {
  const fixtures: Record<string, MockOracleFixture> = {};

  for (const spec of fixtureSpecs(baseTimestamp)) {
    const pubkey = deterministicPubkey(spec.label);
    const owner = PROGRAM_IDS[spec.owner];
    const file = path.join(fixturesDir, `${pubkey.toBase58()}.json`);

    fixtures[spec.label] = {
      file,
      feedIdHex: FEED_IDS[spec.feed],
      label: spec.label,
      owner: owner.toBase58(),
      priceS6: spec.priceS6,
      pubkey: pubkey.toBase58(),
      publishSlot: spec.publishSlot,
      publishTs: spec.publishTs,
    };
  }

  return {
    accountCount: Object.keys(fixtures).length,
    baseTimestamp,
    fixtures,
  };
}

export function writeMockOracleFixtures(
  fixturesDir: string,
  manifestPath: string,
  baseTimestamp: number
): MockOracleManifest {
  fs.mkdirSync(fixturesDir, { recursive: true });

  const manifest = buildMockOracleManifest(baseTimestamp, fixturesDir);

  for (const spec of fixtureSpecs(baseTimestamp)) {
    const pubkey = new PublicKey(manifest.fixtures[spec.label].pubkey);
    const owner = PROGRAM_IDS[spec.owner];
    const data = encodeMockPriceAccount(spec);
    fs.writeFileSync(
      manifest.fixtures[spec.label].file,
      accountJson(pubkey, owner, data)
    );
  }

  fs.mkdirSync(path.dirname(manifestPath), { recursive: true });
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
  return manifest;
}

export function loadMockOracleManifest(manifestPath: string): MockOracleManifest {
  return JSON.parse(fs.readFileSync(manifestPath, "utf8")) as MockOracleManifest;
}

function readArg(flag: string): string | undefined {
  const index = process.argv.indexOf(flag);
  if (index === -1) {
    return undefined;
  }
  return process.argv[index + 1];
}

function main(): void {
  const fixturesDir = readArg("--write-fixtures");
  const manifestPath = readArg("--manifest");

  if (!fixturesDir || !manifestPath) {
    throw new Error(
      "usage: mock_pyth.js --write-fixtures <dir> --manifest <path> [--base-ts <unix>]"
    );
  }

  const baseTimestamp = Number(readArg("--base-ts") ?? Math.floor(Date.now() / 1000));
  const manifest = writeMockOracleFixtures(fixturesDir, manifestPath, baseTimestamp);
  process.stdout.write(JSON.stringify(manifest, null, 2));
}

if (require.main === module) {
  main();
}
