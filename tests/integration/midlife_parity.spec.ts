import * as anchor from "@coral-xyz/anchor";
import { assert, expect } from "chai";
import { createHash } from "crypto";
import fs from "fs";
import path from "path";
import {
  ComputeBudgetProgram,
  Keypair,
  PublicKey,
  SYSVAR_CLOCK_PUBKEY,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
} from "@solana/web3.js";

type MidlifeInputs = {
  current_spy_s6: number;
  current_qqq_s6: number;
  current_iwm_s6: number;
  sigma_common_s6: number;
  entry_spy_s6: number;
  entry_qqq_s6: number;
  entry_iwm_s6: number;
  beta_spy_s12: number;
  beta_qqq_s12: number;
  alpha_s12: number;
  regression_residual_vol_s6: number;
  monthly_coupon_schedule: number[];
  quarterly_autocall_schedule: number[];
  next_coupon_index: number;
  next_autocall_index: number;
  offered_coupon_bps_s6: number;
  coupon_barrier_bps: number;
  autocall_barrier_bps: number;
  ki_barrier_bps: number;
  ki_latched: boolean;
  missed_coupon_observations: number;
  coupons_paid_usdc: number;
  notional_usdc: number;
  now_trading_day: number;
};

type MidlifeFixture = {
  label: string;
  inputs: MidlifeInputs;
  expected_nav_s6: number;
  expected_ki_level_usd_s6: number;
};

type MidlifeFixtureFile = {
  schema_version: number;
  reference_fn: string;
  quadrature: string;
  vectors: MidlifeFixture[];
};

type DebugMidlifeNav = {
  nav_s6: number;
  ki_level_usd_s6: number;
  remaining_coupon_pv_s6: number;
  par_recovery_probability_s6: number;
};

type ParityReportEntry = {
  advance_units_consumed: number[];
  advance_leg_count: number;
  checkpoint_chunk_size: number;
  index: number;
  abs_diff_s6: number;
  expected_nav_s6: number;
  finish_units_consumed: number;
  label: string;
  nav_s6: number;
  prepare_units_consumed: number;
  signed_diff_s6: number;
  transaction_count: number;
  units_consumed: number;
};

type ParityFailureEntry = {
  index: number;
  label: string;
  error: string;
  checkpoint_chunk_size: number | null;
  units_consumed: number | null;
  exceeded_cu: boolean;
};

type SimulationSuccess = {
  ok: true;
  returnData: Buffer;
  unitsConsumed: number;
};

type SimulationFailure = {
  ok: false;
  error: string;
  unitsConsumed: number | null;
  exceededCu: boolean;
};

type LegSuccess = {
  ok: true;
  returnData: Buffer | null;
  unitsConsumed: number;
};

const PROGRAM_ID = new PublicKey(
  "E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc"
);
const FIXTURES_PATH = path.resolve(
  process.cwd(),
  "crates/halcyon_flagship_quote/tests/fixtures/midlife_nav_vectors.json"
);
function sanitizeReportSuffix(value: string): string {
  return value.replace(/[^A-Za-z0-9_.-]/g, "_");
}

function filteredReportSuffix(): string {
  const explicit = process.env.MIDLIFE_PARITY_REPORT_SUFFIX;
  if (explicit && explicit.length > 0) {
    return sanitizeReportSuffix(explicit.startsWith("_") ? explicit : `_${explicit}`);
  }

  const parts: string[] = [];
  if (process.env.MIDLIFE_PARITY_ONLY_LABEL) {
    parts.push("label");
  }
  if (process.env.MIDLIFE_PARITY_ONLY_INDEX) {
    parts.push(`index_${process.env.MIDLIFE_PARITY_ONLY_INDEX}`);
  }
  if (process.env.MIDLIFE_PARITY_INDEX_FROM) {
    parts.push(`from_${process.env.MIDLIFE_PARITY_INDEX_FROM}`);
  }
  if (process.env.MIDLIFE_PARITY_INDEX_TO) {
    parts.push(`to_${process.env.MIDLIFE_PARITY_INDEX_TO}`);
  }

  return parts.length > 0 ? `_${sanitizeReportSuffix(parts.join("_"))}` : "";
}

const REPORT_SUFFIX = filteredReportSuffix();
const REPORT_PATH = path.resolve(
  process.cwd(),
  `.anchor/integration/midlife_parity_report${REPORT_SUFFIX}.json`
);
const RESEARCH_REPORT_PATH = path.resolve(
  process.cwd(),
  `research/midlife_parity_report${REPORT_SUFFIX}.json`
);
const INPUT_SIZE = 340;
const RETURN_SIZE = 32;
const MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE = 20_706;
const MIDLIFE_CHECKPOINT_CHUNK_SIZE = Number(
  process.env.MIDLIFE_CHECKPOINT_CHUNK_SIZE ?? "18"
);
const MIDLIFE_CHECKPOINT_TARGET_UNITS = Number(
  process.env.MIDLIFE_CHECKPOINT_TARGET_UNITS ?? "1280000"
);
const INTEGRATION_MOCHA_TIMEOUT_MS = Number(
  process.env.INTEGRATION_MOCHA_TIMEOUT_MS ?? "3600000"
);
const MIDLIFE_PARITY_PROGRESS_INTERVAL = Number(
  process.env.MIDLIFE_PARITY_PROGRESS_INTERVAL ?? "25"
);
const MAX_ACCEPTABLE_ABS_DIFF_S6 = 1_000;
const MAX_ACCEPTABLE_UNITS_CONSUMED = MIDLIFE_CHECKPOINT_TARGET_UNITS;
const FIXTURE_LABEL_FILTER = process.env.MIDLIFE_PARITY_ONLY_LABEL;
const FIXTURE_INDEX_FILTER = process.env.MIDLIFE_PARITY_ONLY_INDEX
  ? Number(process.env.MIDLIFE_PARITY_ONLY_INDEX)
  : undefined;
const FIXTURE_INDEX_FROM = process.env.MIDLIFE_PARITY_INDEX_FROM
  ? Number(process.env.MIDLIFE_PARITY_INDEX_FROM)
  : undefined;
const FIXTURE_INDEX_TO = process.env.MIDLIFE_PARITY_INDEX_TO
  ? Number(process.env.MIDLIFE_PARITY_INDEX_TO)
  : undefined;

function instructionDiscriminator(name: string): Buffer {
  return createHash("sha256")
    .update(`global:${name}`)
    .digest()
    .subarray(0, 8);
}

assert(
  Number.isInteger(MIDLIFE_CHECKPOINT_CHUNK_SIZE) &&
    MIDLIFE_CHECKPOINT_CHUNK_SIZE >= 1 &&
    MIDLIFE_CHECKPOINT_CHUNK_SIZE <= 18,
  "MIDLIFE_CHECKPOINT_CHUNK_SIZE must be an integer in [1, 18]"
);
assert(
  Number.isInteger(MIDLIFE_CHECKPOINT_TARGET_UNITS) &&
    MIDLIFE_CHECKPOINT_TARGET_UNITS > 0 &&
    MIDLIFE_CHECKPOINT_TARGET_UNITS <= 1_400_000,
  "MIDLIFE_CHECKPOINT_TARGET_UNITS must be an integer in [1, 1400000]"
);
assert(
  Number.isInteger(INTEGRATION_MOCHA_TIMEOUT_MS) &&
    INTEGRATION_MOCHA_TIMEOUT_MS >= 1_000_000,
  "INTEGRATION_MOCHA_TIMEOUT_MS must be an integer >= 1000000"
);
assert(
  Number.isInteger(MIDLIFE_PARITY_PROGRESS_INTERVAL) &&
    MIDLIFE_PARITY_PROGRESS_INTERVAL >= 0,
  "MIDLIFE_PARITY_PROGRESS_INTERVAL must be a non-negative integer"
);
assert(
  FIXTURE_INDEX_FROM === undefined ||
    (Number.isInteger(FIXTURE_INDEX_FROM) && FIXTURE_INDEX_FROM >= 0),
  "MIDLIFE_PARITY_INDEX_FROM must be a non-negative integer"
);
assert(
  FIXTURE_INDEX_TO === undefined ||
    (Number.isInteger(FIXTURE_INDEX_TO) && FIXTURE_INDEX_TO >= 0),
  "MIDLIFE_PARITY_INDEX_TO must be a non-negative integer"
);

function writeI64LE(buffer: Buffer, value: number, offset: number): number {
  buffer.writeBigInt64LE(BigInt(value), offset);
  return offset + 8;
}

function writeU64LE(buffer: Buffer, value: number, offset: number): number {
  buffer.writeBigUInt64LE(BigInt(value), offset);
  return offset + 8;
}

function writeI128LE(buffer: Buffer, value: number, offset: number): number {
  let remaining = BigInt(value);
  const modulo = 1n << 128n;
  if (remaining < 0) {
    remaining += modulo;
  }
  for (let index = 0; index < 16; index += 1) {
    buffer[offset + index] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  return offset + 16;
}

function writeU16LE(buffer: Buffer, value: number, offset: number): number {
  buffer.writeUInt16LE(value, offset);
  return offset + 2;
}

function writeU8(buffer: Buffer, value: number, offset: number): number {
  buffer.writeUInt8(value, offset);
  return offset + 1;
}

function writeBool(buffer: Buffer, value: boolean, offset: number): number {
  return writeU8(buffer, value ? 1 : 0, offset);
}

function encodeDebugMidlifeInputs(inputs: MidlifeInputs): Buffer {
  const buffer = Buffer.alloc(INPUT_SIZE);
  let offset = 0;

  offset = writeI64LE(buffer, inputs.current_spy_s6, offset);
  offset = writeI64LE(buffer, inputs.current_qqq_s6, offset);
  offset = writeI64LE(buffer, inputs.current_iwm_s6, offset);
  offset = writeI64LE(buffer, inputs.sigma_common_s6, offset);
  offset = writeI64LE(buffer, inputs.entry_spy_s6, offset);
  offset = writeI64LE(buffer, inputs.entry_qqq_s6, offset);
  offset = writeI64LE(buffer, inputs.entry_iwm_s6, offset);
  offset = writeI128LE(buffer, inputs.beta_spy_s12, offset);
  offset = writeI128LE(buffer, inputs.beta_qqq_s12, offset);
  offset = writeI128LE(buffer, inputs.alpha_s12, offset);
  offset = writeI64LE(buffer, inputs.regression_residual_vol_s6, offset);

  assert.strictEqual(inputs.monthly_coupon_schedule.length, 18);
  for (const day of inputs.monthly_coupon_schedule) {
    offset = writeI64LE(buffer, day, offset);
  }

  assert.strictEqual(inputs.quarterly_autocall_schedule.length, 6);
  for (const day of inputs.quarterly_autocall_schedule) {
    offset = writeI64LE(buffer, day, offset);
  }

  offset = writeU8(buffer, inputs.next_coupon_index, offset);
  offset = writeU8(buffer, inputs.next_autocall_index, offset);
  offset = writeI64LE(buffer, inputs.offered_coupon_bps_s6, offset);
  offset = writeU16LE(buffer, inputs.coupon_barrier_bps, offset);
  offset = writeU16LE(buffer, inputs.autocall_barrier_bps, offset);
  offset = writeU16LE(buffer, inputs.ki_barrier_bps, offset);
  offset = writeBool(buffer, inputs.ki_latched, offset);
  offset = writeU8(buffer, inputs.missed_coupon_observations, offset);
  offset = writeU64LE(buffer, inputs.coupons_paid_usdc, offset);
  offset = writeU64LE(buffer, inputs.notional_usdc, offset);
  offset = writeU16LE(buffer, inputs.now_trading_day, offset);

  assert.strictEqual(offset, INPUT_SIZE);
  return buffer;
}

function decodeDebugMidlifeNav(returnData: Buffer): DebugMidlifeNav {
  assert.strictEqual(returnData.length, RETURN_SIZE);
  let offset = 0;
  const readI64 = (): number => {
    const value = Number(returnData.readBigInt64LE(offset));
    offset += 8;
    return value;
  };

  return {
    nav_s6: readI64(),
    ki_level_usd_s6: readI64(),
    remaining_coupon_pv_s6: readI64(),
    par_recovery_probability_s6: readI64(),
  };
}

function decodeMidlifeCheckpointPreview(returnData: Buffer): {
  nextCouponIndex: number;
} {
  assert(
    returnData.length >= 2,
    `checkpoint preview return data too short: ${returnData.length}`
  );
  return {
    nextCouponIndex: returnData.readUInt8(0),
  };
}

function debugMidlifeNavIx(inputs: MidlifeInputs): TransactionInstruction {
  const data = Buffer.concat([
    instructionDiscriminator("debug_midlife_nav"),
    encodeDebugMidlifeInputs(inputs),
  ]);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [],
    data,
  });
}

function checkpointChunkCandidates(maxChunkSize: number): number[] {
  return [...new Set([maxChunkSize, 12, 9, 6, 4, 3, 2, 1])]
    .filter((chunkSize) => chunkSize >= 1 && chunkSize <= maxChunkSize)
    .sort((lhs, rhs) => rhs - lhs);
}

function isRetryableCheckpointFailure(failure: ParityFailureEntry): boolean {
  return (
    failure.exceeded_cu ||
    failure.error.includes("ProgramFailedToComplete") ||
    failure.error.includes("memory allocation failed") ||
    failure.error.includes("ComputationalBudgetExceeded") ||
    failure.error.includes("exceeded CUs meter")
  );
}

function nextCheckpointStop(
  inputs: MidlifeInputs,
  currentCouponIndex: number,
  chunkSize: number
): number {
  return Math.min(
    inputs.monthly_coupon_schedule.length,
    currentCouponIndex + chunkSize
  );
}

function debugMidlifeNavPrepareIx(
  requester: PublicKey,
  checkpoint: PublicKey,
  inputs: MidlifeInputs,
  stopCouponIndex: number
): TransactionInstruction {
  const data = Buffer.concat([
    instructionDiscriminator("debug_midlife_nav_prepare"),
    encodeDebugMidlifeInputs(inputs),
    Buffer.from([stopCouponIndex]),
  ]);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: requester, isSigner: true, isWritable: false },
      { pubkey: checkpoint, isSigner: false, isWritable: true },
      { pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false },
    ],
    data,
  });
}

function debugMidlifeNavAdvanceIx(
  requester: PublicKey,
  checkpoint: PublicKey,
  stopCouponIndex: number
): TransactionInstruction {
  const data = Buffer.concat([
    instructionDiscriminator("debug_midlife_nav_advance"),
    Buffer.from([stopCouponIndex]),
  ]);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: requester, isSigner: true, isWritable: false },
      { pubkey: checkpoint, isSigner: false, isWritable: true },
      { pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false },
    ],
    data,
  });
}

function debugMidlifeNavFinishIx(
  requester: PublicKey,
  checkpoint: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: requester, isSigner: true, isWritable: true },
      { pubkey: checkpoint, isSigner: false, isWritable: true },
      { pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false },
    ],
    data: instructionDiscriminator("debug_midlife_nav_finish"),
  });
}

async function buildDebugPrepareTx(
  provider: anchor.AnchorProvider,
  payer: Keypair,
  checkpoint: Keypair,
  inputs: MidlifeInputs,
  stopCouponIndex: number
): Promise<Transaction> {
  const recentBlockhash = await provider.connection.getLatestBlockhash("confirmed");
  const lamports =
    await provider.connection.getMinimumBalanceForRentExemption(
      MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE
    );
  return new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(
    ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: checkpoint.publicKey,
      lamports,
      space: MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE,
      programId: PROGRAM_ID,
    }),
    debugMidlifeNavPrepareIx(
      payer.publicKey,
      checkpoint.publicKey,
      inputs,
      stopCouponIndex
    )
  );
}

async function buildSingleIxTx(
  provider: anchor.AnchorProvider,
  payer: Keypair,
  ix: TransactionInstruction
): Promise<Transaction> {
  const recentBlockhash = await provider.connection.getLatestBlockhash("confirmed");
  return new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }), ix);
}

async function simulateTransactionLeg(
  provider: anchor.AnchorProvider,
  tx: Transaction,
  signers: Keypair[]
): Promise<LegSuccess | SimulationFailure> {
  const result = await simulateLegacyTransaction(provider, tx, signers);
  const logs = result.value.logs ?? [];
  const parsedUnitsConsumed = parseUnitsConsumed(logs);
  const returnData = parseProgramReturn(logs);
  if (result.value.err) {
    const errorMessage = `simulation failed: ${JSON.stringify(result.value.err)}\n${
      logs.join("\n") ?? ""
    }`;
    return {
      ok: false,
      error: errorMessage,
      unitsConsumed: result.value.unitsConsumed ?? parsedUnitsConsumed,
      exceededCu:
        errorMessage.includes("exceeded CUs meter") ||
        errorMessage.includes("ComputationalBudgetExceeded"),
    };
  }
  return {
    ok: true,
    returnData,
    unitsConsumed: result.value.unitsConsumed ?? parsedUnitsConsumed ?? 0,
  };
}

async function sendSignedTransaction(
  provider: anchor.AnchorProvider,
  tx: Transaction,
  signers: Keypair[]
): Promise<void> {
  tx.sign(...signers);
  const signature = await provider.connection.sendRawTransaction(tx.serialize(), {
    skipPreflight: false,
  });
  await provider.connection.confirmTransaction(signature, "confirmed");
}

async function prepareDebugCheckpoint(
  provider: anchor.AnchorProvider,
  payer: Keypair,
  inputs: MidlifeInputs,
  stopCouponIndex: number
): Promise<
  (LegSuccess & { checkpoint: Keypair; nextCouponIndex: number }) | SimulationFailure
> {
  const checkpoint = Keypair.generate();
  const simulateTx = await buildDebugPrepareTx(
    provider,
    payer,
    checkpoint,
    inputs,
    stopCouponIndex
  );
  const simulation = await simulateTransactionLeg(provider, simulateTx, [
    payer,
    checkpoint,
  ]);
  if (simulation.ok === false) {
    return simulation;
  }
  if (simulation.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
    return {
      ok: false,
      error: `prepare exceeded soft CU target: ${simulation.unitsConsumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`,
      unitsConsumed: simulation.unitsConsumed,
      exceededCu: true,
    };
  }
  if (!simulation.returnData) {
    return {
      ok: false,
      error: "prepare: missing checkpoint preview return data",
      unitsConsumed: simulation.unitsConsumed,
      exceededCu: false,
    };
  }
  const preview = decodeMidlifeCheckpointPreview(simulation.returnData);
  const sendTx = await buildDebugPrepareTx(
    provider,
    payer,
    checkpoint,
    inputs,
    stopCouponIndex
  );
  try {
    await sendSignedTransaction(provider, sendTx, [payer, checkpoint]);
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
      unitsConsumed: simulation.unitsConsumed,
      exceededCu: false,
    };
  }
  return {
    ok: true,
    checkpoint,
    nextCouponIndex: preview.nextCouponIndex,
    returnData: simulation.returnData,
    unitsConsumed: simulation.unitsConsumed,
  };
}

async function advanceDebugCheckpoint(
  provider: anchor.AnchorProvider,
  payer: Keypair,
  checkpoint: PublicKey,
  stopCouponIndex: number
): Promise<(LegSuccess & { nextCouponIndex: number }) | SimulationFailure> {
  const ix = debugMidlifeNavAdvanceIx(payer.publicKey, checkpoint, stopCouponIndex);
  const simulateTx = await buildSingleIxTx(provider, payer, ix);
  const simulation = await simulateTransactionLeg(provider, simulateTx, [payer]);
  if (simulation.ok === false) {
    return simulation;
  }
  if (simulation.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
    return {
      ok: false,
      error: `advance exceeded soft CU target: ${simulation.unitsConsumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`,
      unitsConsumed: simulation.unitsConsumed,
      exceededCu: true,
    };
  }
  if (!simulation.returnData) {
    return {
      ok: false,
      error: "advance: missing checkpoint preview return data",
      unitsConsumed: simulation.unitsConsumed,
      exceededCu: false,
    };
  }
  const preview = decodeMidlifeCheckpointPreview(simulation.returnData);
  const sendTx = await buildSingleIxTx(provider, payer, ix);
  try {
    await sendSignedTransaction(provider, sendTx, [payer]);
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
      unitsConsumed: simulation.unitsConsumed,
      exceededCu: false,
    };
  }
  return {
    ...simulation,
    nextCouponIndex: preview.nextCouponIndex,
  };
}

async function simulateViewBufferWithBudget(
  provider: anchor.AnchorProvider,
  payer: Keypair,
  ix: TransactionInstruction
): Promise<SimulationSuccess | SimulationFailure> {
  const recentBlockhash = await provider.connection.getLatestBlockhash("confirmed");
  const tx = new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }), ix);
  const result = await simulateLegacyTransaction(provider, tx, [payer]);
  const logs = result.value.logs ?? [];
  const parsedUnitsConsumed = parseUnitsConsumed(logs);
  if (result.value.err) {
    const errorMessage = `simulation failed: ${JSON.stringify(result.value.err)}\n${
      logs.join("\n") ?? ""
    }`;
    return {
      ok: false,
      error: errorMessage,
      unitsConsumed: result.value.unitsConsumed ?? parsedUnitsConsumed,
      exceededCu:
        errorMessage.includes("exceeded CUs meter") ||
        errorMessage.includes("ComputationalBudgetExceeded"),
    };
  }

  const prefix = `Program return: ${PROGRAM_ID.toBase58()} `;
  const returnLog = logs.find((log) => log.startsWith(prefix));
  if (!returnLog) {
    return {
      ok: false,
      error: `missing return log for ${PROGRAM_ID.toBase58()}`,
      unitsConsumed: result.value.unitsConsumed ?? parsedUnitsConsumed,
      exceededCu: false,
    };
  }
  const unitsConsumed = result.value.unitsConsumed ?? parsedUnitsConsumed;
  assert(unitsConsumed !== null);
  return {
    ok: true,
    returnData: Buffer.from(returnLog.slice(prefix.length), "base64"),
    unitsConsumed,
  };
}

async function simulateLegacyTransaction(
  provider: anchor.AnchorProvider,
  tx: Transaction,
  signers: Keypair[]
) {
  const recentBlockhash = await provider.connection.getLatestBlockhash("confirmed");
  const feePayer = tx.feePayer ?? signers[0]?.publicKey;
  assert(feePayer, "simulation transaction needs a fee payer");
  const message = new TransactionMessage({
    payerKey: feePayer,
    recentBlockhash: recentBlockhash.blockhash,
    instructions: tx.instructions,
  }).compileToV0Message();
  const versioned = new VersionedTransaction(message);
  versioned.sign(signers);
  return provider.connection.simulateTransaction(versioned, {
    commitment: "confirmed",
    replaceRecentBlockhash: false,
    sigVerify: true,
  });
}

function parseUnitsConsumed(logs: string[]): number | null {
  let unitsConsumed: number | null = null;
  for (const log of logs) {
    const match = log.match(/consumed (\d+) of (\d+) compute units/);
    if (!match) {
      continue;
    }
    const parsed = Number(match[1]);
    if (Number.isFinite(parsed)) {
      unitsConsumed = unitsConsumed === null ? parsed : Math.max(unitsConsumed, parsed);
    }
  }
  return unitsConsumed;
}

function parseProgramReturn(logs: string[]): Buffer | null {
  const prefix = `Program return: ${PROGRAM_ID.toBase58()} `;
  const returnLog = logs.find((log) => log.startsWith(prefix));
  return returnLog ? Buffer.from(returnLog.slice(prefix.length), "base64") : null;
}

async function runCheckpointedDebugFixture(
  provider: anchor.AnchorProvider,
  payer: Keypair,
  fixture: MidlifeFixture,
  index: number,
  chunkSize: number
): Promise<
  | { ok: true; entry: ParityReportEntry }
  | { ok: false; failure: ParityFailureEntry }
> {
  let currentCouponIndex = fixture.inputs.next_coupon_index;
  const initialStop = nextCheckpointStop(
    fixture.inputs,
    currentCouponIndex,
    chunkSize
  );
  const prepared = await prepareDebugCheckpoint(
    provider,
    payer,
    fixture.inputs,
    initialStop
  );
  if (prepared.ok === false) {
    return {
      ok: false,
      failure: {
        index,
        label: fixture.label,
        error: `prepare: ${prepared.error}`,
        checkpoint_chunk_size: chunkSize,
        units_consumed: prepared.unitsConsumed,
        exceeded_cu: prepared.exceededCu,
      },
    };
  }

  currentCouponIndex = prepared.nextCouponIndex;
  const advanceUnits: number[] = [];
  let advanceLegCount = 0;
  while (currentCouponIndex < fixture.inputs.monthly_coupon_schedule.length) {
    advanceLegCount += 1;
    if (advanceLegCount > 64) {
      return {
        ok: false,
        failure: {
          index,
          label: fixture.label,
          error: "advance: checkpoint made no bounded progress after 64 legs",
          checkpoint_chunk_size: chunkSize,
          units_consumed: null,
          exceeded_cu: false,
        },
      };
    }
    const nextStop = nextCheckpointStop(
      fixture.inputs,
      currentCouponIndex,
      chunkSize
    );
    const advanced = await advanceDebugCheckpoint(
      provider,
      payer,
      prepared.checkpoint.publicKey,
      nextStop
    );
    if (advanced.ok === false) {
      return {
        ok: false,
        failure: {
          index,
          label: fixture.label,
          error: `advance to ${nextStop}: ${advanced.error}`,
          checkpoint_chunk_size: chunkSize,
          units_consumed: advanced.unitsConsumed,
          exceeded_cu: advanced.exceededCu,
        },
      };
    }
    advanceUnits.push(advanced.unitsConsumed);
    currentCouponIndex = advanced.nextCouponIndex;
  }

  const simulation = await simulateViewBufferWithBudget(
    provider,
    payer,
    debugMidlifeNavFinishIx(payer.publicKey, prepared.checkpoint.publicKey)
  );
  if (simulation.ok === false) {
    return {
      ok: false,
      failure: {
        index,
        label: fixture.label,
        error: `finish: ${simulation.error}`,
        checkpoint_chunk_size: chunkSize,
        units_consumed: simulation.unitsConsumed,
        exceeded_cu: simulation.exceededCu,
      },
    };
  }
  if (simulation.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
    return {
      ok: false,
      failure: {
        index,
        label: fixture.label,
        error: `finish exceeded soft CU target: ${simulation.unitsConsumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`,
        checkpoint_chunk_size: chunkSize,
        units_consumed: simulation.unitsConsumed,
        exceeded_cu: true,
      },
    };
  }
  const nav = decodeDebugMidlifeNav(simulation.returnData);
  if (nav.ki_level_usd_s6 !== fixture.expected_ki_level_usd_s6) {
    return {
      ok: false,
      failure: {
        index,
        label: fixture.label,
        error: `ki level mismatch: got ${nav.ki_level_usd_s6} expected ${fixture.expected_ki_level_usd_s6}`,
        checkpoint_chunk_size: chunkSize,
        units_consumed: simulation.unitsConsumed,
        exceeded_cu: false,
      },
    };
  }

  return {
    ok: true,
    entry: {
      advance_units_consumed: advanceUnits,
      advance_leg_count: advanceLegCount,
      checkpoint_chunk_size: chunkSize,
      index,
      abs_diff_s6: Math.abs(nav.nav_s6 - fixture.expected_nav_s6),
      expected_nav_s6: fixture.expected_nav_s6,
      finish_units_consumed: simulation.unitsConsumed,
      label: fixture.label,
      nav_s6: nav.nav_s6,
      prepare_units_consumed: prepared.unitsConsumed,
      signed_diff_s6: nav.nav_s6 - fixture.expected_nav_s6,
      transaction_count: 2 + advanceLegCount,
      units_consumed: Math.max(
        prepared.unitsConsumed,
        simulation.unitsConsumed,
        ...advanceUnits
      ),
    },
  };
}

function percentile(values: number[], p: number): number {
  if (values.length === 0) {
    return 0;
  }
  const sorted = [...values].sort((lhs, rhs) => lhs - rhs);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(sorted.length * p) - 1)
  );
  return sorted[index];
}

describe("midlife on-chain parity", function () {
  this.timeout(INTEGRATION_MOCHA_TIMEOUT_MS);

  it("matches the committed 300-vector host grid via the SBF debug view", async () => {
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);
    const payer = (provider.wallet as anchor.Wallet).payer;

    const fixtureFile = JSON.parse(
      fs.readFileSync(FIXTURES_PATH, "utf8")
    ) as MidlifeFixtureFile;
    expect(fixtureFile.reference_fn).to.eq("nav_c1_filter_mid_life");
    expect(fixtureFile.quadrature).to.eq("GH9");
    expect(fixtureFile.vectors.length).to.eq(300);
    const selectedVectors = fixtureFile.vectors.filter((fixture, index) => {
      if (FIXTURE_LABEL_FILTER && fixture.label !== FIXTURE_LABEL_FILTER) {
        return false;
      }
      if (FIXTURE_INDEX_FILTER !== undefined && index !== FIXTURE_INDEX_FILTER) {
        return false;
      }
      if (FIXTURE_INDEX_FROM !== undefined && index < FIXTURE_INDEX_FROM) {
        return false;
      }
      if (FIXTURE_INDEX_TO !== undefined && index >= FIXTURE_INDEX_TO) {
        return false;
      }
      return true;
    });
    expect(selectedVectors.length).to.be.greaterThan(
      0,
      "midlife parity filter matched no fixtures"
    );

    const entries: ParityReportEntry[] = [];
    const failures: ParityFailureEntry[] = [];
    const chunkCandidates = checkpointChunkCandidates(MIDLIFE_CHECKPOINT_CHUNK_SIZE);
    for (const fixture of selectedVectors) {
      const index = fixtureFile.vectors.findIndex(
        (candidate) => candidate.label === fixture.label
      );
      let lastFailure: ParityFailureEntry | null = null;
      let succeeded = false;
      for (const chunkSize of chunkCandidates) {
        const result = await runCheckpointedDebugFixture(
          provider,
          payer,
          fixture,
          index,
          chunkSize
        );
        if (result.ok) {
          if (result.entry.units_consumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
            lastFailure = {
              index,
              label: fixture.label,
              error: `checkpoint chunk ${chunkSize} exceeded soft CU target: ${result.entry.units_consumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`,
              checkpoint_chunk_size: chunkSize,
              units_consumed: result.entry.units_consumed,
              exceeded_cu: true,
            };
            continue;
          }
          entries.push(result.entry);
          succeeded = true;
          break;
        }
        lastFailure = result.failure;
        if (!isRetryableCheckpointFailure(result.failure)) {
          break;
        }
      }
      if (!succeeded) {
        failures.push(
          lastFailure ?? {
            index,
            label: fixture.label,
            error: "checkpointed debug run failed without a captured error",
            checkpoint_chunk_size: null,
            units_consumed: null,
            exceeded_cu: false,
          }
        );
      }
      const completed = entries.length + failures.length;
      if (
        MIDLIFE_PARITY_PROGRESS_INTERVAL > 0 &&
        completed % MIDLIFE_PARITY_PROGRESS_INTERVAL === 0
      ) {
        console.log(
          `midlife parity progress completed=${completed}/${selectedVectors.length} ok=${entries.length} failed=${failures.length}`
        );
      }
    }

    const diffs = entries.map((entry) => entry.abs_diff_s6);
    const signedDiffs = entries.map((entry) => entry.signed_diff_s6);
    const units = entries.map((entry) => entry.units_consumed);
    const chunkSizeCounts = entries.reduce<Record<string, number>>((counts, entry) => {
      const key = String(entry.checkpoint_chunk_size);
      counts[key] = (counts[key] ?? 0) + 1;
      return counts;
    }, {});
    const failedUnits = failures
      .map((entry) => entry.units_consumed)
      .filter((value): value is number => value !== null);
    const worst = [...entries]
      .sort((lhs, rhs) => rhs.abs_diff_s6 - lhs.abs_diff_s6)
      .slice(0, 10);
    const worstPositive = [...entries]
      .filter((entry) => entry.signed_diff_s6 > 0)
      .sort((lhs, rhs) => rhs.signed_diff_s6 - lhs.signed_diff_s6)
      .slice(0, 10);
    const worstNegative = [...entries]
      .filter((entry) => entry.signed_diff_s6 < 0)
      .sort((lhs, rhs) => lhs.signed_diff_s6 - rhs.signed_diff_s6)
      .slice(0, 10);
    const maxAbsDiff = diffs.length > 0 ? Math.max(...diffs) : null;
    const p95AbsDiff = diffs.length > 0 ? percentile(diffs, 0.95) : null;
    const maxUnits = units.length > 0 ? Math.max(...units) : null;
    const p95Units = units.length > 0 ? percentile(units, 0.95) : null;
    const transactionCounts = entries.map((entry) => entry.transaction_count);
    const minSignedDiff =
      signedDiffs.length > 0 ? Math.min(...signedDiffs) : null;
    const understatementCount = signedDiffs.filter((value) => value < 0).length;
    const report = {
      fixture_count: selectedVectors.length,
      successful_fixture_count: entries.length,
      failed_fixture_count: failures.length,
      cu_exceeded_fixture_count: failures.filter((entry) => entry.exceeded_cu).length,
      compute_unit_hard_limit: 1_400_000,
      compute_unit_soft_target: MIDLIFE_CHECKPOINT_TARGET_UNITS,
      compute_unit_headroom_at_max:
        maxUnits === null ? null : 1_400_000 - maxUnits,
      compute_unit_soft_headroom_at_max:
        maxUnits === null ? null : MIDLIFE_CHECKPOINT_TARGET_UNITS - maxUnits,
      fixture_label_filter: FIXTURE_LABEL_FILTER ?? null,
      fixture_index_filter: FIXTURE_INDEX_FILTER ?? null,
      fixture_index_from: FIXTURE_INDEX_FROM ?? null,
      fixture_index_to: FIXTURE_INDEX_TO ?? null,
      checkpoint_candidate_chunks: chunkCandidates,
      checkpoint_planner:
        "preflight_simulates_candidate_chunks_before_send; no fee-burning failed-send retry is used",
      max_checkpoint_transaction_count:
        transactionCounts.length > 0 ? Math.max(...transactionCounts) : null,
      p95_checkpoint_transaction_count:
        transactionCounts.length > 0 ? percentile(transactionCounts, 0.95) : null,
      pricing_method: "deterministic_monthly_c1_checkpoint",
      residual_definition:
        "signed_diff_s6 = raw on-chain NAV minus committed host reference NAV; no fixture lookup, interpolation table, or NAV correction is applied",
      overstated_count: signedDiffs.filter((value) => value > 0).length,
      understated_count: understatementCount,
      understatement_rate:
        entries.length > 0 ? understatementCount / entries.length : null,
      worst_understatement_s6:
        minSignedDiff !== null && minSignedDiff < 0 ? minSignedDiff : 0,
      exact_match_count: signedDiffs.filter((value) => value === 0).length,
      max_abs_diff_s6: maxAbsDiff,
      p95_abs_diff_s6: p95AbsDiff,
      max_abs_diff_bps_at_nav_1:
        maxAbsDiff === null ? null : maxAbsDiff / 100,
      p95_abs_diff_bps_at_nav_1:
        p95AbsDiff === null ? null : p95AbsDiff / 100,
      mean_abs_diff_s6:
        diffs.length > 0
          ? diffs.reduce((sum, value) => sum + value, 0) / diffs.length
          : null,
      max_signed_diff_s6: signedDiffs.length > 0 ? Math.max(...signedDiffs) : null,
      min_signed_diff_s6: minSignedDiff,
      mean_signed_diff_s6:
        signedDiffs.length > 0
          ? signedDiffs.reduce((sum, value) => sum + value, 0) / signedDiffs.length
          : null,
      max_units_consumed: maxUnits,
      p95_units_consumed: p95Units,
      checkpoint_chunk_size_counts: chunkSizeCounts,
      max_failed_units_consumed:
        failedUnits.length > 0 ? Math.max(...failedUnits) : null,
      successful_cases: entries,
      worst_cases: worst,
      worst_positive_cases: worstPositive,
      worst_negative_cases: worstNegative,
      failed_cases: failures,
    };

    fs.mkdirSync(path.dirname(REPORT_PATH), { recursive: true });
    const reportJson = `${JSON.stringify(report, null, 2)}\n`;
    fs.writeFileSync(REPORT_PATH, reportJson);
    fs.mkdirSync(path.dirname(RESEARCH_REPORT_PATH), { recursive: true });
    fs.writeFileSync(RESEARCH_REPORT_PATH, reportJson);
    console.log(
      `midlife parity fixtures=${report.fixture_count} ok=${report.successful_fixture_count} failed=${report.failed_fixture_count} cu_exceeded=${report.cu_exceeded_fixture_count} overstated=${report.overstated_count} understated=${report.understated_count} exact=${report.exact_match_count} max_abs_diff_s6=${report.max_abs_diff_s6 ?? "n/a"} p95_abs_diff_s6=${report.p95_abs_diff_s6 ?? "n/a"} max_cu=${report.max_units_consumed ?? "n/a"} p95_cu=${report.p95_units_consumed ?? "n/a"}`
    );
    for (const entry of worst) {
      console.log(
        `midlife parity worst index=${entry.index} label=${entry.label} diff_s6=${entry.abs_diff_s6} nav_s6=${entry.nav_s6} expected_s6=${entry.expected_nav_s6} cu=${entry.units_consumed}`
      );
    }
    for (const failure of failures.slice(0, 10)) {
      console.log(
        `midlife parity failed index=${failure.index} label=${failure.label} exceeded_cu=${failure.exceeded_cu} cu=${failure.units_consumed ?? "n/a"}`
      );
    }

    expect(
      report.failed_fixture_count,
      `midlife parity had ${report.failed_fixture_count} failing fixtures; see ${REPORT_PATH}`
    ).to.eq(0);

    if (report.max_abs_diff_s6 !== null) {
      expect(
        report.max_abs_diff_s6,
        `max abs diff exceeded ${MAX_ACCEPTABLE_ABS_DIFF_S6} s6; see ${REPORT_PATH}`
      ).to.be.at.most(MAX_ACCEPTABLE_ABS_DIFF_S6);
    }

    // Check the per-transaction CU envelope across the checkpointed path.
    // `units_consumed` is the max leg CU, not the sum across continuation
    // transactions.
    if (report.max_units_consumed !== null) {
      expect(
        report.max_units_consumed,
        `max CU exceeded ${MAX_ACCEPTABLE_UNITS_CONSUMED}; worst case in ${REPORT_PATH}`
      ).to.be.at.most(MAX_ACCEPTABLE_UNITS_CONSUMED);
    }
  });
});
