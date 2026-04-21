import { Connection, PublicKey } from "@solana/web3.js";

const SYSVAR_CLOCK_PUBKEY = new PublicKey(
  "SysvarC1ock11111111111111111111111111111111"
);

type ClockSnapshot = {
  slot: bigint;
  unixTimestamp: number;
};

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function readClock(
  connection: Connection
): Promise<ClockSnapshot> {
  const info = await connection.getAccountInfo(SYSVAR_CLOCK_PUBKEY, "processed");
  if (!info) {
    throw new Error("clock sysvar account not found");
  }
  return {
    slot: info.data.readBigUInt64LE(0),
    unixTimestamp: Number(info.data.readBigInt64LE(32)),
  };
}

export async function advanceTime(
  connection: Connection,
  seconds: number
): Promise<ClockSnapshot> {
  const start = await readClock(connection);
  const targetUnixTimestamp = start.unixTimestamp + seconds;

  for (;;) {
    const current = await readClock(connection);
    if (current.unixTimestamp >= targetUnixTimestamp) {
      return current;
    }
    const remainingSecs = Math.max(1, targetUnixTimestamp - current.unixTimestamp);
    await sleep(Math.min(remainingSecs * 250, 500));
  }
}
