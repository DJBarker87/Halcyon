import { BN } from "@coral-xyz/anchor";
import {
  AccountMeta,
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
import { assert, expect } from "chai";
import { createHash } from "crypto";
import {
  getAccount,
  getAssociatedTokenAddressSync,
  TOKEN_PROGRAM_ID,
} from "../kernel/token_harness";

import { setupFullProtocol, TestContext } from "./setup";

const SEEDS = {
  autocallSchedule: Buffer.from("autocall_schedule"),
  policy: Buffer.from("policy"),
  policyReceipt: Buffer.from("policy_receipt"),
  policyReceiptAuthority: Buffer.from("policy_receipt_authority"),
  policyReceiptMint: Buffer.from("policy_receipt_mint"),
  terms: Buffer.from("terms"),
} as const;

const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
);
const MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE = 20_706;
const MIDLIFE_CHECKPOINT_CHUNK_SIZE = Number(
  process.env.MIDLIFE_CHECKPOINT_CHUNK_SIZE ?? "18"
);
const MIDLIFE_CHECKPOINT_TARGET_UNITS = Number(
  process.env.MIDLIFE_CHECKPOINT_TARGET_UNITS ?? "1280000"
);
const MIDLIFE_FINAL_COUPON_INDEX = 18;
const MIDLIFE_INITIAL_COUPON_INDEX = 0;

function pda(
  seeds: Buffer[],
  programId: PublicKey
): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, programId)[0];
}

function instructionDiscriminator(name: string): Buffer {
  return createHash("sha256")
    .update(`global:${name}`)
    .digest()
    .subarray(0, 8);
}

assert(
  Number.isInteger(MIDLIFE_CHECKPOINT_CHUNK_SIZE) &&
    MIDLIFE_CHECKPOINT_CHUNK_SIZE >= 1 &&
    MIDLIFE_CHECKPOINT_CHUNK_SIZE <= MIDLIFE_FINAL_COUPON_INDEX,
  "MIDLIFE_CHECKPOINT_CHUNK_SIZE must be an integer in [1, 18]"
);
assert(
  Number.isInteger(MIDLIFE_CHECKPOINT_TARGET_UNITS) &&
    MIDLIFE_CHECKPOINT_TARGET_UNITS > 0 &&
    MIDLIFE_CHECKPOINT_TARGET_UNITS <= 1_400_000,
  "MIDLIFE_CHECKPOINT_TARGET_UNITS must be an integer in [1, 1400000]"
);

function nextMidlifeCheckpointStop(currentCouponIndex: number): number {
  return Math.min(
    MIDLIFE_FINAL_COUPON_INDEX,
    currentCouponIndex + MIDLIFE_CHECKPOINT_CHUNK_SIZE
  );
}
function checkpointChunkCandidates(maxChunkSize: number): number[] {
  return [...new Set([maxChunkSize, 12, 9, 6, 4, 3, 2, 1])]
    .filter((chunkSize) => chunkSize >= 1 && chunkSize <= maxChunkSize)
    .sort((lhs, rhs) => rhs - lhs);
}

function nextMidlifeCheckpointStopForChunk(
  currentCouponIndex: number,
  chunkSize: number
): number {
  return Math.min(
    MIDLIFE_FINAL_COUPON_INDEX,
    currentCouponIndex + chunkSize
  );
}

async function simulateUnitsConsumed(
  ctx: TestContext,
  payer: Keypair,
  ix: TransactionInstruction
): Promise<number> {
  const recentBlockhash = await ctx.provider.connection.getLatestBlockhash(
    "confirmed"
  );
  const tx = new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(
    ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
    ix
  );
  const result = await simulateLegacyTransaction(ctx, tx, [payer]);
  if (result.value.err) {
    throw new Error(
      `simulation failed: ${JSON.stringify(result.value.err)}\n${
        result.value.logs?.join("\n") ?? ""
      }`
    );
  }
  assert(result.value.unitsConsumed !== undefined);
  return result.value.unitsConsumed ?? 0;
}

async function simulateViewBufferWithBudget(
  ctx: TestContext,
  programId: PublicKey,
  payer: Keypair,
  ix: TransactionInstruction
): Promise<{ returnData: Buffer; unitsConsumed: number }> {
  const simulation = await trySimulateViewBufferWithBudget(
    ctx,
    programId,
    payer,
    ix
  );
  if (!simulation.ok) {
    throw new Error(simulation.error);
  }
  return {
    returnData: simulation.returnData,
    unitsConsumed: simulation.unitsConsumed,
  };
}

function lastProgramReturnData(
  logs: string[] | null | undefined,
  programId: PublicKey
): Buffer | null {
  const returnPrefix = `Program return: ${programId.toBase58()} `;
  const returnLogs = logs?.filter((log) => log.startsWith(returnPrefix)) ?? [];
  const returnLog = returnLogs.at(-1);
  return returnLog
    ? Buffer.from(returnLog.slice(returnPrefix.length), "base64")
    : null;
}

async function trySimulateViewBufferWithBudget(
  ctx: TestContext,
  programId: PublicKey,
  payer: Keypair,
  ix: TransactionInstruction
): Promise<
  | { ok: true; returnData: Buffer; unitsConsumed: number }
  | { ok: false; error: string; unitsConsumed: number | null }
> {
  const recentBlockhash = await ctx.provider.connection.getLatestBlockhash(
    "confirmed"
  );
  const tx = new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(
    ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
    ix
  );
  const result = await simulateLegacyTransaction(ctx, tx, [payer]);
  if (result.value.err) {
    return {
      ok: false,
      error: `simulation failed: ${JSON.stringify(result.value.err)}\n${
        result.value.logs?.join("\n") ?? ""
      }`,
      unitsConsumed: result.value.unitsConsumed ?? null,
    };
  }

  const returnData = lastProgramReturnData(result.value.logs, programId);
  if (!returnData) {
    return {
      ok: false,
      error: `missing return log for ${programId.toBase58()}`,
      unitsConsumed: result.value.unitsConsumed ?? null,
    };
  }

  assert(result.value.unitsConsumed !== undefined);
  return {
    ok: true,
    returnData,
    unitsConsumed: result.value.unitsConsumed ?? 0,
  };
}

async function simulateReturnBufferTx(
  ctx: TestContext,
  programId: PublicKey,
  tx: Transaction,
  signers: Keypair[]
): Promise<{ returnData: Buffer; unitsConsumed: number }> {
  const result = await simulateLegacyTransaction(ctx, tx, signers);
  if (result.value.err) {
    throw new Error(
      `simulation failed: ${JSON.stringify(result.value.err)}\n${
        result.value.logs?.join("\n") ?? ""
      }`
    );
  }
  const returnData = lastProgramReturnData(result.value.logs, programId);
  if (!returnData) {
    throw new Error(`missing return log for ${programId.toBase58()}`);
  }
  assert(result.value.unitsConsumed !== undefined);
  return {
    returnData,
    unitsConsumed: result.value.unitsConsumed ?? 0,
  };
}

async function simulateViewWithBudget<T>(
  ctx: TestContext,
  program: any,
  payer: Keypair,
  ix: TransactionInstruction,
  instructionName: string
): Promise<{ unitsConsumed: number; value: T }> {
  const { returnData, unitsConsumed } = await simulateViewBufferWithBudget(
    ctx,
    program.programId,
    payer,
    ix
  );
  const idlIx = program.idl.instructions.find(
    (candidate: any) => candidate.name === instructionName
  );
  if (!idlIx?.returns) {
    throw new Error(`missing IDL return type for ${instructionName}`);
  }
  const { IdlCoder } = require("@coral-xyz/anchor/dist/cjs/coder/borsh/idl");
  const coder = IdlCoder.fieldLayout({ type: idlIx.returns }, program.idl.types);
  const value = coder.decode(returnData) as T;
  return {
    unitsConsumed,
    value,
  };
}

async function simulateTransactionUnits(
  ctx: TestContext,
  tx: Transaction,
  signers: Keypair[]
): Promise<number> {
  const result = await simulateLegacyTransaction(ctx, tx, signers);
  if (result.value.err) {
    throw new Error(
      `simulation failed: ${JSON.stringify(result.value.err)}\n${
        result.value.logs?.join("\n") ?? ""
      }`
    );
  }
  assert(result.value.unitsConsumed !== undefined);
  return result.value.unitsConsumed ?? 0;
}

async function simulateCheckpointTx(
  ctx: TestContext,
  programId: PublicKey,
  tx: Transaction,
  signers: Keypair[]
): Promise<{ nextCouponIndex: number; unitsConsumed: number }> {
  const result = await simulateLegacyTransaction(ctx, tx, signers);
  if (result.value.err) {
    throw new Error(
      `simulation failed: ${JSON.stringify(result.value.err)}\n${
        result.value.logs?.join("\n") ?? ""
      }`
    );
  }

  const returnData = lastProgramReturnData(result.value.logs, programId);
  if (!returnData) {
    throw new Error(`missing checkpoint preview return log for ${programId.toBase58()}`);
  }
  assert(returnData.length >= 2);
  assert(result.value.unitsConsumed !== undefined);
  return {
    nextCouponIndex: returnData.readUInt8(0),
    unitsConsumed: result.value.unitsConsumed ?? 0,
  };
}

async function buildCheckpointBatchTx(
  ctx: TestContext,
  payer: Keypair,
  instructions: TransactionInstruction[]
): Promise<Transaction> {
  const recentBlockhash = await ctx.provider.connection.getLatestBlockhash(
    "confirmed"
  );
  return new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(...instructions);
}

async function simulateLegacyTransaction(
  ctx: TestContext,
  tx: Transaction,
  signers: Keypair[]
) {
  const recentBlockhash = await ctx.provider.connection.getLatestBlockhash(
    "confirmed"
  );
  const feePayer = tx.feePayer ?? signers[0]?.publicKey;
  assert(feePayer, "simulation transaction needs a fee payer");
  const message = new TransactionMessage({
    payerKey: feePayer,
    recentBlockhash: recentBlockhash.blockhash,
    instructions: tx.instructions,
  }).compileToV0Message();
  const versioned = new VersionedTransaction(message);
  versioned.sign(signers);
  return ctx.provider.connection.simulateTransaction(versioned, {
    commitment: "confirmed",
    replaceRecentBlockhash: false,
    sigVerify: true,
  });
}

async function sendSignedTransaction(
  ctx: TestContext,
  tx: Transaction,
  signers: Keypair[]
): Promise<void> {
  tx.sign(...signers);
  const signature = await ctx.provider.connection.sendRawTransaction(
    tx.serialize(),
    { skipPreflight: false }
  );
  await ctx.provider.connection.confirmTransaction(signature, "confirmed");
}

async function buildPrepareMidlifeCheckpointTx(
  ctx: TestContext,
  flagshipProgram: any,
  payer: Keypair,
  checkpoint: Keypair,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
    protocolConfig: PublicKey;
    pythIwm: PublicKey;
    pythQqq: PublicKey;
    pythSpy: PublicKey;
    regression: PublicKey;
    vaultSigma: PublicKey;
  },
  stopCouponIndex: number
): Promise<Transaction> {
  const recentBlockhash = await ctx.provider.connection.getLatestBlockhash(
    "confirmed"
  );
  const lamports =
    await ctx.provider.connection.getMinimumBalanceForRentExemption(
      MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE
    );
  const createCheckpointIx = SystemProgram.createAccount({
    fromPubkey: payer.publicKey,
    newAccountPubkey: checkpoint.publicKey,
    lamports,
    space: MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE,
    programId: flagshipProgram.programId,
  });
  const prepareIx = flagshipPrepareMidlifeNavIx(
    flagshipProgram.programId,
    payer.publicKey,
    checkpoint.publicKey,
    accounts,
    stopCouponIndex
  );

  return new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(
    ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
    createCheckpointIx,
    prepareIx
  );
}

function flagshipPrepareMidlifeNavIx(
  programId: PublicKey,
  requester: PublicKey,
  checkpoint: PublicKey,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
    protocolConfig: PublicKey;
    pythIwm: PublicKey;
    pythQqq: PublicKey;
    pythSpy: PublicKey;
    regression: PublicKey;
    vaultSigma: PublicKey;
  },
  stopCouponIndex: number
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: requester, isSigner: true, isWritable: false },
      { pubkey: checkpoint, isSigner: false, isWritable: true },
      { pubkey: accounts.protocolConfig, isSigner: false, isWritable: false },
      { pubkey: accounts.vaultSigma, isSigner: false, isWritable: false },
      { pubkey: accounts.regression, isSigner: false, isWritable: false },
      { pubkey: accounts.policyHeader, isSigner: false, isWritable: false },
      { pubkey: accounts.productTerms, isSigner: false, isWritable: false },
      { pubkey: accounts.pythSpy, isSigner: false, isWritable: false },
      { pubkey: accounts.pythQqq, isSigner: false, isWritable: false },
      { pubkey: accounts.pythIwm, isSigner: false, isWritable: false },
      { pubkey: accounts.clock, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([
      instructionDiscriminator("prepare_midlife_nav"),
      Buffer.from([stopCouponIndex]),
    ]),
  });
}

function flagshipAdvanceMidlifeNavIx(
  programId: PublicKey,
  requester: PublicKey,
  checkpoint: PublicKey,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
  },
  stopCouponIndex: number
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: requester, isSigner: true, isWritable: false },
      { pubkey: checkpoint, isSigner: false, isWritable: true },
      { pubkey: accounts.policyHeader, isSigner: false, isWritable: false },
      { pubkey: accounts.productTerms, isSigner: false, isWritable: false },
      { pubkey: accounts.clock, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([
      instructionDiscriminator("advance_midlife_nav"),
      Buffer.from([stopCouponIndex]),
    ]),
  });
}

function flagshipPreviewLendingValueFromCheckpointIx(
  programId: PublicKey,
  requester: PublicKey,
  checkpoint: PublicKey,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
  }
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: requester, isSigner: true, isWritable: true },
      { pubkey: checkpoint, isSigner: false, isWritable: true },
      { pubkey: accounts.policyHeader, isSigner: false, isWritable: false },
      { pubkey: accounts.productTerms, isSigner: false, isWritable: false },
      { pubkey: accounts.clock, isSigner: false, isWritable: false },
    ],
    data: instructionDiscriminator("preview_lending_value_from_checkpoint"),
  });
}

async function buildAdvanceMidlifeCheckpointTx(
  ctx: TestContext,
  flagshipProgram: any,
  payer: Keypair,
  checkpoint: PublicKey,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
  },
  stopCouponIndex: number
): Promise<Transaction> {
  const recentBlockhash = await ctx.provider.connection.getLatestBlockhash(
    "confirmed"
  );
  return new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(
    ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
    flagshipAdvanceMidlifeNavIx(
      flagshipProgram.programId,
      payer.publicKey,
      checkpoint,
      accounts,
      stopCouponIndex
    )
  );
}

async function prepareInitialMidlifeCheckpoint(
  ctx: TestContext,
  flagshipProgram: any,
  payer: Keypair,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
    protocolConfig: PublicKey;
    pythIwm: PublicKey;
    pythQqq: PublicKey;
    pythSpy: PublicKey;
    regression: PublicKey;
    vaultSigma: PublicKey;
  },
  startCouponIndex = MIDLIFE_INITIAL_COUPON_INDEX
): Promise<{
  checkpoint: Keypair;
  prepareChunkSize: number;
  nextCouponIndex: number;
  unitsConsumed: number;
}> {
  const checkpoint = Keypair.generate();
  let currentCouponIndex = startCouponIndex;
  const chunkCandidates = checkpointChunkCandidates(MIDLIFE_CHECKPOINT_CHUNK_SIZE);
  let preparedPreview:
    | { chunkSize: number; nextCouponIndex: number; unitsConsumed: number }
    | null = null;
  let lastPrepareError: unknown = null;
  for (const chunkSize of chunkCandidates) {
    const initialStop = nextMidlifeCheckpointStopForChunk(
      currentCouponIndex,
      chunkSize
    );
    const simulateTx = await buildPrepareMidlifeCheckpointTx(
      ctx,
      flagshipProgram,
      payer,
      checkpoint,
      accounts,
      initialStop
    );
    try {
      const preview = await simulateCheckpointTx(
        ctx,
        flagshipProgram.programId,
        simulateTx,
        [payer, checkpoint]
      );
      if (preview.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
        lastPrepareError = new Error(
          `prepare checkpoint chunk ${chunkSize} exceeded soft CU target: ${preview.unitsConsumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`
        );
        continue;
      }
      preparedPreview = { chunkSize, ...preview };
      break;
    } catch (error) {
      lastPrepareError = error;
    }
  }
  if (preparedPreview === null) {
    throw new Error(
      `prepare checkpoint failed for chunks ${chunkCandidates.join(",")}: ${
        lastPrepareError instanceof Error
          ? lastPrepareError.message
          : String(lastPrepareError)
      }`
    );
  }
  const unitsConsumed = preparedPreview.unitsConsumed;

  const sendTx = await buildPrepareMidlifeCheckpointTx(
    ctx,
    flagshipProgram,
    payer,
    checkpoint,
    accounts,
    nextMidlifeCheckpointStopForChunk(
      currentCouponIndex,
      preparedPreview.chunkSize
    )
  );
  await sendSignedTransaction(ctx, sendTx, [payer, checkpoint]);

  currentCouponIndex = preparedPreview.nextCouponIndex;
  return {
    checkpoint,
    prepareChunkSize: preparedPreview.chunkSize,
    nextCouponIndex: currentCouponIndex,
    unitsConsumed,
  };
}

async function prepareMidlifeCheckpoint(
  ctx: TestContext,
  flagshipProgram: any,
  payer: Keypair,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
    protocolConfig: PublicKey;
    pythIwm: PublicKey;
    pythQqq: PublicKey;
    pythSpy: PublicKey;
    regression: PublicKey;
    vaultSigma: PublicKey;
  },
  startCouponIndex = MIDLIFE_INITIAL_COUPON_INDEX
): Promise<{
  advanceUnitsConsumed: number[];
  advanceChunkSizes: number[];
  checkpoint: Keypair;
  prepareChunkSize: number;
  unitsConsumed: number;
}> {
  const prepared = await prepareInitialMidlifeCheckpoint(
    ctx,
    flagshipProgram,
    payer,
    accounts,
    startCouponIndex
  );
  const checkpoint = prepared.checkpoint;
  let currentCouponIndex = prepared.nextCouponIndex;
  const chunkCandidates = checkpointChunkCandidates(MIDLIFE_CHECKPOINT_CHUNK_SIZE);
  const unitsConsumed = prepared.unitsConsumed;
  const advanceUnitsConsumed: number[] = [];
  const advanceChunkSizes: number[] = [];
  let advanceLegCount = 0;
  while (currentCouponIndex < MIDLIFE_FINAL_COUPON_INDEX) {
    advanceLegCount += 1;
    assert(advanceLegCount <= 64, "checkpoint did not make bounded progress");
    const advanceAccounts = {
      clock: accounts.clock,
      policyHeader: accounts.policyHeader,
      productTerms: accounts.productTerms,
    };
    let advancedPreview:
      | { chunkSize: number; nextCouponIndex: number; unitsConsumed: number }
      | null = null;
    let lastAdvanceError: unknown = null;
    for (const chunkSize of chunkCandidates) {
      const nextStop = nextMidlifeCheckpointStopForChunk(
        currentCouponIndex,
        chunkSize
      );
      const simulateAdvanceTx = await buildAdvanceMidlifeCheckpointTx(
        ctx,
        flagshipProgram,
        payer,
        checkpoint.publicKey,
        advanceAccounts,
        nextStop
      );
      try {
        const preview = await simulateCheckpointTx(
          ctx,
          flagshipProgram.programId,
          simulateAdvanceTx,
          [payer]
        );
        if (preview.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
          lastAdvanceError = new Error(
            `advance checkpoint chunk ${chunkSize} exceeded soft CU target: ${preview.unitsConsumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`
          );
          continue;
        }
        advancedPreview = { chunkSize, ...preview };
        break;
      } catch (error) {
        lastAdvanceError = error;
      }
    }
    if (advancedPreview === null) {
      throw new Error(
        `advance checkpoint failed at coupon ${currentCouponIndex} for chunks ${chunkCandidates.join(",")}: ${
          lastAdvanceError instanceof Error
            ? lastAdvanceError.message
            : String(lastAdvanceError)
        }`
      );
    }
    const advanceUnits = advancedPreview.unitsConsumed;
    advanceUnitsConsumed.push(advanceUnits);
    advanceChunkSizes.push(advancedPreview.chunkSize);

    const sendAdvanceTx = await buildAdvanceMidlifeCheckpointTx(
      ctx,
      flagshipProgram,
      payer,
      checkpoint.publicKey,
      advanceAccounts,
      nextMidlifeCheckpointStopForChunk(
        currentCouponIndex,
        advancedPreview.chunkSize
      )
    );
    await sendSignedTransaction(ctx, sendAdvanceTx, [payer]);
    currentCouponIndex = advancedPreview.nextCouponIndex;
  }

  return {
    advanceUnitsConsumed,
    advanceChunkSizes,
    checkpoint,
    prepareChunkSize: prepared.prepareChunkSize,
    unitsConsumed: Math.max(unitsConsumed, ...advanceUnitsConsumed),
  };
}

function asSafeNumber(value: bigint, label: string): number {
  const numberValue = Number(value);
  assert(
    Number.isSafeInteger(numberValue),
    `${label} exceeded Number safe integer range`
  );
  return numberValue;
}

function decodeLendingValuePreview(returnData: Buffer): {
  navS6: number;
  kiLevelUsdS6: number;
  lendingValueS6: number;
  lendingValuePayoutUsdc: number;
  remainingCouponPvS6: number;
  parRecoveryProbabilityS6: number;
  sigmaPricingS6: number;
  nowTradingDay: number;
} {
  let offset = 0;
  const readI64 = (label: string): number => {
    const value = returnData.readBigInt64LE(offset);
    offset += 8;
    return asSafeNumber(value, label);
  };
  const readU64 = (label: string): number => {
    const value = returnData.readBigUInt64LE(offset);
    offset += 8;
    return asSafeNumber(value, label);
  };
  const readU16 = (): number => {
    const value = returnData.readUInt16LE(offset);
    offset += 2;
    return value;
  };

  return {
    navS6: readI64("navS6"),
    kiLevelUsdS6: readI64("kiLevelUsdS6"),
    lendingValueS6: readI64("lendingValueS6"),
    lendingValuePayoutUsdc: readU64("lendingValuePayoutUsdc"),
    remainingCouponPvS6: readI64("remainingCouponPvS6"),
    parRecoveryProbabilityS6: readI64("parRecoveryProbabilityS6"),
    sigmaPricingS6: readI64("sigmaPricingS6"),
    nowTradingDay: readU16(),
  };
}

function lendingValuePreviewToNumbers(value: any): {
  navS6: number;
  kiLevelUsdS6: number;
  lendingValueS6: number;
  lendingValuePayoutUsdc: number;
  remainingCouponPvS6: number;
  parRecoveryProbabilityS6: number;
  sigmaPricingS6: number;
  nowTradingDay: number;
} {
  return {
    navS6: Number(value.navS6.toString()),
    kiLevelUsdS6: Number(value.kiLevelUsdS6.toString()),
    lendingValueS6: Number(value.lendingValueS6.toString()),
    lendingValuePayoutUsdc: Number(value.lendingValuePayoutUsdc.toString()),
    remainingCouponPvS6: Number(value.remainingCouponPvS6.toString()),
    parRecoveryProbabilityS6: Number(
      value.parRecoveryProbabilityS6.toString()
    ),
    sigmaPricingS6: Number(value.sigmaPricingS6.toString()),
    nowTradingDay: Number(value.nowTradingDay),
  };
}

function firstSimulationErrorLine(error: string): string {
  return error.split("\n").find((line) => line.length > 0) ?? error;
}

async function simulateCheckpointedLendingValue(
  ctx: TestContext,
  flagshipProgram: any,
  payer: Keypair,
  checkpoint: PublicKey,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
  }
): Promise<{
  preview: ReturnType<typeof decodeLendingValuePreview>;
  unitsConsumed: number;
}> {
  const ix = flagshipPreviewLendingValueFromCheckpointIx(
    flagshipProgram.programId,
    payer.publicKey,
    checkpoint,
    accounts
  );
  const { returnData, unitsConsumed } = await simulateViewBufferWithBudget(
    ctx,
    flagshipProgram.programId,
    payer,
    ix
  );
  return {
    preview: decodeLendingValuePreview(returnData),
    unitsConsumed,
  };
}

async function simulatePackedCheckpointedLendingValue(
  ctx: TestContext,
  flagshipProgram: any,
  payer: Keypair,
  checkpoint: PublicKey,
  startCouponIndex: number,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
  }
): Promise<{
  preview: ReturnType<typeof decodeLendingValuePreview>;
  finishUnitsConsumed: number;
  transactionUnitsConsumed: number[];
  unitsConsumed: number;
  advanceChunkSizes: number[];
  transactionCount: number;
}> {
  const advanceAccounts = {
    clock: accounts.clock,
    policyHeader: accounts.policyHeader,
    productTerms: accounts.productTerms,
  };
  const chunkCandidates = checkpointChunkCandidates(MIDLIFE_CHECKPOINT_CHUNK_SIZE);
  const budgetIx = ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 });
  let instructions: TransactionInstruction[] = [
    budgetIx,
  ];
  const transactionUnitsConsumed: number[] = [];
  const sendPlannedAdvanceBatch = async (): Promise<void> => {
    if (instructions.length <= 1) {
      return;
    }
    const tx = await buildCheckpointBatchTx(ctx, payer, instructions);
    const unitsConsumed = await simulateTransactionUnits(ctx, tx, [payer]);
    assert(
      unitsConsumed <= MIDLIFE_CHECKPOINT_TARGET_UNITS,
      `packed checkpoint advance batch exceeded soft CU target: ${unitsConsumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`
    );
    await sendSignedTransaction(ctx, tx, [payer]);
    transactionUnitsConsumed.push(unitsConsumed);
    instructions = [
      budgetIx,
    ];
  };
  const advanceChunkSizes: number[] = [];
  let currentCouponIndex = startCouponIndex;
  let guard = 0;

  while (currentCouponIndex < MIDLIFE_FINAL_COUPON_INDEX) {
    guard += 1;
    assert(guard <= 64, "packed checkpoint did not make bounded progress");
    let selected:
      | {
          chunkSize: number;
          ix: TransactionInstruction;
          nextCouponIndex: number;
        }
      | null = null;

    for (const chunkSize of chunkCandidates) {
      const nextStop = nextMidlifeCheckpointStopForChunk(
        currentCouponIndex,
        chunkSize
      );
      const ix = flagshipAdvanceMidlifeNavIx(
        flagshipProgram.programId,
        payer.publicKey,
        checkpoint,
        advanceAccounts,
        nextStop
      );
      const tx = await buildCheckpointBatchTx(ctx, payer, [...instructions, ix]);
      try {
        const preview = await simulateCheckpointTx(
          ctx,
          flagshipProgram.programId,
          tx,
          [payer]
        );
        if (preview.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
          continue;
        }
        selected = { chunkSize, ix, nextCouponIndex: preview.nextCouponIndex };
        break;
      } catch {
        // Try the next-smaller deterministic chunk.
      }
    }

    if (selected === null) {
      if (instructions.length <= 1) {
        throw new Error(
          `packed advance failed at coupon ${currentCouponIndex} for chunks ${chunkCandidates.join(",")}`
        );
      }
      await sendPlannedAdvanceBatch();
      continue;
    }
    instructions.push(selected.ix);
    advanceChunkSizes.push(selected.chunkSize);
    currentCouponIndex = selected.nextCouponIndex;
  }

  const finishIx = flagshipPreviewLendingValueFromCheckpointIx(
    flagshipProgram.programId,
    payer.publicKey,
    checkpoint,
    accounts
  );
  let finishTx = await buildCheckpointBatchTx(ctx, payer, [
    ...instructions,
    finishIx,
  ]);
  let finishResult: { returnData: Buffer; unitsConsumed: number } | null = null;
  try {
    finishResult = await simulateReturnBufferTx(
      ctx,
      flagshipProgram.programId,
      finishTx,
      [payer]
    );
    if (finishResult.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
      finishResult = null;
    }
  } catch {
    finishResult = null;
  }
  if (finishResult === null && instructions.length > 1) {
    await sendPlannedAdvanceBatch();
    finishTx = await buildCheckpointBatchTx(ctx, payer, [
      budgetIx,
      finishIx,
    ]);
    finishResult = await simulateReturnBufferTx(
      ctx,
      flagshipProgram.programId,
      finishTx,
      [payer]
    );
  }
  assert(finishResult !== null, "packed checkpoint finish failed");
  const { returnData, unitsConsumed } = finishResult;
  assert(
    unitsConsumed <= MIDLIFE_CHECKPOINT_TARGET_UNITS,
    `packed checkpoint finish exceeded soft CU target: ${unitsConsumed} > ${MIDLIFE_CHECKPOINT_TARGET_UNITS}`
  );
  const packedUnitsConsumed = [...transactionUnitsConsumed, unitsConsumed];
  return {
    preview: decodeLendingValuePreview(returnData),
    finishUnitsConsumed: unitsConsumed,
    transactionUnitsConsumed: packedUnitsConsumed,
    unitsConsumed: Math.max(...packedUnitsConsumed),
    advanceChunkSizes,
    transactionCount: 1 + packedUnitsConsumed.length,
  };
}

function flagshipPreviewLendingValueIx(
  programId: PublicKey,
  accounts: {
    clock: PublicKey;
    policyHeader: PublicKey;
    productTerms: PublicKey;
    protocolConfig: PublicKey;
    pythIwm: PublicKey;
    pythQqq: PublicKey;
    pythSpy: PublicKey;
    regression: PublicKey;
    vaultSigma: PublicKey;
  }
): TransactionInstruction {
  const keys: AccountMeta[] = [
    { pubkey: accounts.protocolConfig, isSigner: false, isWritable: false },
    { pubkey: accounts.vaultSigma, isSigner: false, isWritable: false },
    { pubkey: accounts.regression, isSigner: false, isWritable: false },
    { pubkey: accounts.policyHeader, isSigner: false, isWritable: false },
    { pubkey: accounts.productTerms, isSigner: false, isWritable: false },
    { pubkey: accounts.pythSpy, isSigner: false, isWritable: false },
    { pubkey: accounts.pythQqq, isSigner: false, isWritable: false },
    { pubkey: accounts.pythIwm, isSigner: false, isWritable: false },
    { pubkey: accounts.clock, isSigner: false, isWritable: false },
  ];

  return new TransactionInstruction({
    programId,
    keys,
    data: Buffer.from("d75420684ac08fe8", "hex"),
  });
}

function receiptAccounts(
  kernelProgramId: PublicKey,
  policyHeader: PublicKey,
  holder: PublicKey
): {
  holderReceiptToken: PublicKey;
  policyReceipt: PublicKey;
  receiptAuthority: PublicKey;
  receiptMint: PublicKey;
} {
  const receiptMint = pda(
    [SEEDS.policyReceiptMint, policyHeader.toBuffer()],
    kernelProgramId
  );
  return {
    holderReceiptToken: getAssociatedTokenAddressSync(receiptMint, holder),
    policyReceipt: pda(
      [SEEDS.policyReceipt, policyHeader.toBuffer()],
      kernelProgramId
    ),
    receiptAuthority: pda(
      [SEEDS.policyReceiptAuthority, policyHeader.toBuffer()],
      kernelProgramId
    ),
    receiptMint,
  };
}

function kernelWrapPolicyReceiptIx(
  kernelProgramId: PublicKey,
  currentOwner: PublicKey,
  policyHeader: PublicKey
): TransactionInstruction {
  const receipt = receiptAccounts(kernelProgramId, policyHeader, currentOwner);
  return new TransactionInstruction({
    programId: kernelProgramId,
    keys: [
      { pubkey: currentOwner, isSigner: true, isWritable: true },
      { pubkey: policyHeader, isSigner: false, isWritable: true },
      { pubkey: receipt.policyReceipt, isSigner: false, isWritable: true },
      { pubkey: receipt.receiptMint, isSigner: false, isWritable: true },
      { pubkey: receipt.receiptAuthority, isSigner: false, isWritable: false },
      { pubkey: receipt.holderReceiptToken, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      {
        pubkey: ASSOCIATED_TOKEN_PROGRAM_ID,
        isSigner: false,
        isWritable: false,
      },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: instructionDiscriminator("wrap_policy_receipt"),
  });
}

function kernelUnwrapPolicyReceiptIx(
  kernelProgramId: PublicKey,
  holder: PublicKey,
  policyHeader: PublicKey
): TransactionInstruction {
  const receipt = receiptAccounts(kernelProgramId, policyHeader, holder);
  return new TransactionInstruction({
    programId: kernelProgramId,
    keys: [
      { pubkey: holder, isSigner: true, isWritable: true },
      { pubkey: policyHeader, isSigner: false, isWritable: true },
      { pubkey: receipt.policyReceipt, isSigner: false, isWritable: true },
      { pubkey: receipt.receiptMint, isSigner: false, isWritable: true },
      { pubkey: receipt.receiptAuthority, isSigner: false, isWritable: false },
      { pubkey: receipt.holderReceiptToken, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: instructionDiscriminator("unwrap_policy_receipt"),
  });
}

function splTokenTransferIx(
  source: PublicKey,
  destination: PublicKey,
  owner: PublicKey,
  amount: bigint
): TransactionInstruction {
  const data = Buffer.alloc(9);
  data[0] = 3; // TokenInstruction::Transfer
  data.writeBigUInt64LE(amount, 1);
  return new TransactionInstruction({
    programId: TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: source, isSigner: false, isWritable: true },
      { pubkey: destination, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: true, isWritable: false },
    ],
    data,
  });
}

function createAssociatedTokenAccountIx(
  payer: PublicKey,
  associatedToken: PublicKey,
  owner: PublicKey,
  mint: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: associatedToken, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.alloc(0),
  });
}

function flagshipBuybackFromCheckpointIx(
  programId: PublicKey,
  policyOwner: PublicKey,
  checkpoint: PublicKey,
  accounts: {
    clock: PublicKey;
    kernelProgram: PublicKey;
    ownerUsdc: PublicKey;
    policyHeader: PublicKey;
    productAuthority: PublicKey;
    productRegistryEntry: PublicKey;
    productTerms: PublicKey;
    protocolConfig: PublicKey;
    usdcMint: PublicKey;
    vaultAuthority: PublicKey;
    vaultState: PublicKey;
    vaultUsdc: PublicKey;
  }
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: policyOwner, isSigner: true, isWritable: true },
      { pubkey: checkpoint, isSigner: false, isWritable: true },
      { pubkey: accounts.policyHeader, isSigner: false, isWritable: true },
      { pubkey: accounts.productTerms, isSigner: false, isWritable: true },
      {
        pubkey: accounts.productRegistryEntry,
        isSigner: false,
        isWritable: true,
      },
      { pubkey: accounts.protocolConfig, isSigner: false, isWritable: false },
      { pubkey: accounts.usdcMint, isSigner: false, isWritable: false },
      { pubkey: accounts.vaultUsdc, isSigner: false, isWritable: true },
      { pubkey: accounts.vaultAuthority, isSigner: false, isWritable: false },
      { pubkey: accounts.ownerUsdc, isSigner: false, isWritable: true },
      {
        pubkey: accounts.productAuthority,
        isSigner: false,
        isWritable: false,
      },
      { pubkey: accounts.vaultState, isSigner: false, isWritable: true },
      { pubkey: accounts.clock, isSigner: false, isWritable: false },
      { pubkey: accounts.kernelProgram, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: instructionDiscriminator("buyback_from_checkpoint"),
  });
}

describe("real product integration", function () {
  this.timeout(1_000_000);

  let ctx: TestContext;

  before(async () => {
    ctx = await setupFullProtocol();
  });

  it("boots the protocol and previews IL + flagship products", async () => {
    const protocolConfigInfo = await ctx.provider.connection.getAccountInfo(
      ctx.pdas.protocolConfig,
      "confirmed"
    );
    assert(protocolConfigInfo !== null);
    assert(protocolConfigInfo.owner.equals(ctx.programs.kernel.programId));

    const buyer = ctx.buyers[0];
    const ilPreviewIx = await ctx.programs.ilProtection.methods
      .previewQuote(new BN(5_000_000_000))
      .accounts({
        clock: SYSVAR_CLOCK_PUBKEY,
        productRegistryEntry: ctx.products.il.productRegistryEntry,
        protocolConfig: ctx.pdas.protocolConfig,
        pythSol: new PublicKey(ctx.oracles["il-sol-entry"].pubkey),
        pythUsdc: new PublicKey(ctx.oracles["il-usdc-entry"].pubkey),
        regimeSignal: ctx.products.il.regimeSignal,
        vaultSigma: ctx.products.il.vaultSigma,
      } as any)
      .instruction();
    const { value: ilPreview } = await simulateViewWithBudget<any>(
      ctx,
      ctx.programs.ilProtection,
      buyer.keypair,
      ilPreviewIx,
      "previewQuote"
    );
    expect(new BN(ilPreview.premium).gt(new BN(0))).to.eq(true);
    expect(new BN(ilPreview.maxLiability).gt(new BN(0))).to.eq(true);

    const previewQuoteIx = await ctx.programs.flagshipAutocall.methods
      .previewQuote(new BN(5_000_000_000))
      .accounts({
        autocallSchedule: pda(
          [
            SEEDS.autocallSchedule,
            ctx.programs.flagshipAutocall.programId.toBuffer(),
          ],
          ctx.programs.kernel.programId
        ),
        clock: SYSVAR_CLOCK_PUBKEY,
        productRegistryEntry: ctx.products.flagship.productRegistryEntry,
        protocolConfig: ctx.pdas.protocolConfig,
        pythIwm: new PublicKey(ctx.oracles["flagship-iwm-entry"].pubkey),
        pythQqq: new PublicKey(ctx.oracles["flagship-qqq-entry"].pubkey),
        pythSpy: new PublicKey(ctx.oracles["flagship-spy-entry"].pubkey),
        regression: ctx.products.flagship.regression,
        vaultSigma: ctx.products.flagship.vaultSigma,
      } as any)
      .instruction();
    const { value: flagshipPreview } = await simulateViewWithBudget<any>(
      ctx,
      ctx.programs.flagshipAutocall,
      buyer.keypair,
      previewQuoteIx,
      "previewQuote"
    );
    expect(new BN(flagshipPreview.maxLiability).gt(new BN(0))).to.eq(true);
    expect(new BN(flagshipPreview.entrySpyPriceS6).gt(new BN(0))).to.eq(true);
  });

  it("issues flagship, previews lending value on-chain, and reports CU", async () => {
    const flagshipProgram = ctx.programs.flagshipAutocall as any;
    const buyer = ctx.buyers[0];
    const notionalUsdc = new BN(5_000_000_000);
    const flagshipRegression = ctx.products.flagship.regression;
    assert(flagshipRegression !== undefined, "missing flagship regression PDA");
    const autocallSchedule = pda(
      [
        SEEDS.autocallSchedule,
        ctx.programs.flagshipAutocall.programId.toBuffer(),
      ],
      ctx.programs.kernel.programId
    );
    const previewQuoteAccounts = {
      autocallSchedule,
      clock: SYSVAR_CLOCK_PUBKEY,
      productRegistryEntry: ctx.products.flagship.productRegistryEntry,
      protocolConfig: ctx.pdas.protocolConfig,
      pythIwm: new PublicKey(ctx.oracles["flagship-iwm-entry"].pubkey),
      pythQqq: new PublicKey(ctx.oracles["flagship-qqq-entry"].pubkey),
      pythSpy: new PublicKey(ctx.oracles["flagship-spy-entry"].pubkey),
      regression: ctx.products.flagship.regression!,
      vaultSigma: ctx.products.flagship.vaultSigma,
    } as const;

    const previewQuoteIx = await flagshipProgram.methods
      .previewQuote(notionalUsdc)
      .accounts(previewQuoteAccounts as any)
      .instruction();
    const {
      unitsConsumed: previewQuoteCu,
      value: previewQuote,
    } = await simulateViewWithBudget<any>(
      ctx,
      flagshipProgram,
      buyer.keypair,
      previewQuoteIx,
      "previewQuote"
    );

    const policyId = Keypair.generate().publicKey;
    const policyHeader = pda(
      [SEEDS.policy, policyId.toBuffer()],
      ctx.programs.kernel.programId
    );
    const productTerms = pda(
      [SEEDS.terms, policyId.toBuffer()],
      ctx.programs.flagshipAutocall.programId
    );

    const acceptSignature = await ctx.programs.flagshipAutocall.methods
      .acceptQuote({
        policyId,
        notionalUsdc,
        maxPremium: new BN(previewQuote.premium.toString()),
        minMaxLiability: new BN(previewQuote.maxLiability.toString()),
        minOfferedCouponBpsS6: new BN(
          previewQuote.offeredCouponBpsS6.toString()
        ),
        previewQuoteSlot: new BN(previewQuote.quoteSlot.toString()),
        maxQuoteSlotDelta: new BN(10_000),
        previewEntrySpyPriceS6: new BN(previewQuote.entrySpyPriceS6.toString()),
        previewEntryQqqPriceS6: new BN(previewQuote.entryQqqPriceS6.toString()),
        previewEntryIwmPriceS6: new BN(previewQuote.entryIwmPriceS6.toString()),
        maxEntryPriceDeviationBps: 10,
        previewExpiryTs: new BN(previewQuote.expiryTs.toString()),
        maxExpiryDeltaSecs: new BN(60),
      })
      .accounts({
        buyer: buyer.keypair.publicKey,
        policyHeader,
        productTerms,
        productAuthority: ctx.products.flagship.authority,
        usdcMint: ctx.usdcMint,
        buyerUsdc: buyer.usdc,
        vaultUsdc: ctx.pdas.vaultUsdc,
        treasuryUsdc: ctx.pdas.treasuryUsdc,
        vaultAuthority: ctx.pdas.vaultAuthority,
        protocolConfig: ctx.pdas.protocolConfig,
        vaultSigma: ctx.products.flagship.vaultSigma,
        regression: ctx.products.flagship.regression,
        pythSpy: new PublicKey(ctx.oracles["flagship-spy-entry"].pubkey),
        pythQqq: new PublicKey(ctx.oracles["flagship-qqq-entry"].pubkey),
        pythIwm: new PublicKey(ctx.oracles["flagship-iwm-entry"].pubkey),
        vaultState: ctx.pdas.vaultState,
        feeLedger: ctx.pdas.feeLedger,
        productRegistryEntry: ctx.products.flagship.productRegistryEntry,
        clock: SYSVAR_CLOCK_PUBKEY,
        kernelProgram: ctx.programs.kernel.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        autocallSchedule,
      } as any)
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
      ])
      .signers([buyer.keypair])
      .rpc();
    await ctx.provider.connection.confirmTransaction(
      acceptSignature,
      "confirmed"
    );
    const issuedPolicyInfo = await ctx.provider.connection.getAccountInfo(
      policyHeader,
      "confirmed"
    );
    assert(issuedPolicyInfo !== null, "issued policy header was not confirmed");

    const previewLendingAccounts = {
      clock: SYSVAR_CLOCK_PUBKEY,
      policyHeader,
      productTerms,
      protocolConfig: ctx.pdas.protocolConfig,
      pythIwm: new PublicKey(ctx.oracles["flagship-iwm-entry"].pubkey),
      pythQqq: new PublicKey(ctx.oracles["flagship-qqq-entry"].pubkey),
      pythSpy: new PublicKey(ctx.oracles["flagship-spy-entry"].pubkey),
      regression: flagshipRegression,
      vaultSigma: ctx.products.flagship.vaultSigma,
    } as const;

    const previewLendingIx = flagshipPreviewLendingValueIx(
      flagshipProgram.programId,
      previewLendingAccounts
    );
    const directPreviewLending = await trySimulateViewBufferWithBudget(
      ctx,
      flagshipProgram.programId,
      buyer.keypair,
      previewLendingIx
    );
    const preparedCheckpoint = await prepareMidlifeCheckpoint(
      ctx,
      flagshipProgram,
      buyer.keypair,
      previewLendingAccounts
    );
    const checkpointPreviewResult = await simulateCheckpointedLendingValue(
      ctx,
      flagshipProgram,
      buyer.keypair,
      preparedCheckpoint.checkpoint.publicKey,
      {
        clock: SYSVAR_CLOCK_PUBKEY,
        policyHeader,
        productTerms,
      }
    );
    const lendingPreview = checkpointPreviewResult.preview;

    if (directPreviewLending.ok) {
      const directLendingPreview = decodeLendingValuePreview(
        directPreviewLending.returnData
      );
      expect(directLendingPreview).to.deep.include(lendingPreview);
    }

    console.log(
      `flagship preview_quote CU=${previewQuoteCu} offered_coupon_bps_s6=${previewQuote.offeredCouponBpsS6.toString()} sigma_pricing_s6=${previewQuote.sigmaPricingS6.toString()} direct_preview_lending_value=${
        directPreviewLending.ok
          ? `ok CU=${directPreviewLending.unitsConsumed}`
          : `failed units=${directPreviewLending.unitsConsumed ?? "n/a"} error=${firstSimulationErrorLine(directPreviewLending.error)}`
      } checkpoint_prepare_chunk=${preparedCheckpoint.prepareChunkSize} checkpoint_prepare_max_cu=${preparedCheckpoint.unitsConsumed} checkpoint_advance_chunks=${preparedCheckpoint.advanceChunkSizes.join(",") || "none"} checkpoint_advance_cu=${preparedCheckpoint.advanceUnitsConsumed.join(",") || "none"} checkpoint_finish_cu=${checkpointPreviewResult.unitsConsumed} nav=${lendingPreview.navS6} coupon_pv=${lendingPreview.remainingCouponPvS6} par_recovery=${lendingPreview.parRecoveryProbabilityS6} sigma=${lendingPreview.sigmaPricingS6} now_trading_day=${lendingPreview.nowTradingDay}`
    );

    expect(previewQuoteCu).to.be.greaterThan(0);
    expect(previewQuoteCu).to.be.lessThan(1_400_000);
    expect(preparedCheckpoint.unitsConsumed).to.be.greaterThan(0);
    expect(preparedCheckpoint.unitsConsumed).to.be.at.most(
      MIDLIFE_CHECKPOINT_TARGET_UNITS
    );
    expect(checkpointPreviewResult.unitsConsumed).to.be.greaterThan(0);
    expect(checkpointPreviewResult.unitsConsumed).to.be.at.most(
      MIDLIFE_CHECKPOINT_TARGET_UNITS
    );
    expect(lendingPreview.navS6).to.be.greaterThan(0);

    const r100PreviewLendingAccounts = {
      ...previewLendingAccounts,
      pythSpy: new PublicKey(ctx.oracles["flagship-spy-r100"].pubkey),
      pythQqq: new PublicKey(ctx.oracles["flagship-qqq-r100"].pubkey),
      pythIwm: new PublicKey(ctx.oracles["flagship-iwm-r100"].pubkey),
    } as const;
    const r100PreviewLendingIx = flagshipPreviewLendingValueIx(
      flagshipProgram.programId,
      r100PreviewLendingAccounts
    );
    const r100DirectPreviewLending = await trySimulateViewBufferWithBudget(
      ctx,
      flagshipProgram.programId,
      buyer.keypair,
      r100PreviewLendingIx
    );
    const r100PreparedCheckpoint = await prepareInitialMidlifeCheckpoint(
      ctx,
      flagshipProgram,
      buyer.keypair,
      r100PreviewLendingAccounts
    );
    const r100CheckpointPreviewResult = await simulatePackedCheckpointedLendingValue(
      ctx,
      flagshipProgram,
      buyer.keypair,
      r100PreparedCheckpoint.checkpoint.publicKey,
      r100PreparedCheckpoint.nextCouponIndex,
      {
        clock: SYSVAR_CLOCK_PUBKEY,
        policyHeader,
        productTerms,
      }
    );
    const r100CheckpointPreview = r100CheckpointPreviewResult.preview;

    if (r100DirectPreviewLending.ok) {
      const r100DirectPreview = decodeLendingValuePreview(
        r100DirectPreviewLending.returnData
      );
      expect(r100DirectPreview).to.deep.include(r100CheckpointPreview);
    }

    console.log(
      `flagship r100/coupon0 direct_preview_lending_value=${
        r100DirectPreviewLending.ok
          ? `ok CU=${r100DirectPreviewLending.unitsConsumed}`
          : `failed units=${r100DirectPreviewLending.unitsConsumed ?? "n/a"} error=${firstSimulationErrorLine(r100DirectPreviewLending.error)}`
      } checkpoint_prepare_chunk=${r100PreparedCheckpoint.prepareChunkSize} checkpoint_prepare_cu=${r100PreparedCheckpoint.unitsConsumed} packed_tx_count=${r100CheckpointPreviewResult.transactionCount} packed_tx_cu=${r100CheckpointPreviewResult.transactionUnitsConsumed.join(",")} packed_advance_chunks=${r100CheckpointPreviewResult.advanceChunkSizes.join(",") || "none"} packed_finish_tx_cu=${r100CheckpointPreviewResult.finishUnitsConsumed} packed_max_tx_cu=${r100CheckpointPreviewResult.unitsConsumed} nav=${r100CheckpointPreview.navS6} coupon_pv=${r100CheckpointPreview.remainingCouponPvS6} par_recovery=${r100CheckpointPreview.parRecoveryProbabilityS6} sigma=${r100CheckpointPreview.sigmaPricingS6} offered_coupon_bps_s6=${previewQuote.offeredCouponBpsS6.toString()} now_trading_day=${r100CheckpointPreview.nowTradingDay}`
    );

    expect(r100PreparedCheckpoint.unitsConsumed).to.be.greaterThan(0);
    expect(r100PreparedCheckpoint.unitsConsumed).to.be.at.most(
      MIDLIFE_CHECKPOINT_TARGET_UNITS
    );
    expect(r100CheckpointPreviewResult.unitsConsumed).to.be.greaterThan(0);
    expect(r100CheckpointPreviewResult.unitsConsumed).to.be.at.most(
      MIDLIFE_CHECKPOINT_TARGET_UNITS
    );
    expect(r100CheckpointPreviewResult.transactionCount).to.be.at.most(3);
    expect(r100CheckpointPreview.navS6).to.be.greaterThan(0);
  });

  it("wraps a flagship receipt, posts it as collateral, and liquidates through checkpointed buyback", async () => {
    const flagshipProgram = ctx.programs.flagshipAutocall as any;
    const borrower = ctx.buyers[1];
    const lender = ctx.buyers[0];
    const notionalUsdc = new BN(5_000_000_000);
    const flagshipRegression = ctx.products.flagship.regression;
    assert(flagshipRegression !== undefined, "missing flagship regression PDA");
    const autocallSchedule = pda(
      [
        SEEDS.autocallSchedule,
        ctx.programs.flagshipAutocall.programId.toBuffer(),
      ],
      ctx.programs.kernel.programId
    );
    const previewQuoteAccounts = {
      autocallSchedule,
      clock: SYSVAR_CLOCK_PUBKEY,
      productRegistryEntry: ctx.products.flagship.productRegistryEntry,
      protocolConfig: ctx.pdas.protocolConfig,
      pythIwm: new PublicKey(ctx.oracles["flagship-iwm-entry"].pubkey),
      pythQqq: new PublicKey(ctx.oracles["flagship-qqq-entry"].pubkey),
      pythSpy: new PublicKey(ctx.oracles["flagship-spy-entry"].pubkey),
      regression: ctx.products.flagship.regression!,
      vaultSigma: ctx.products.flagship.vaultSigma,
    } as const;

    const previewQuoteIx = await flagshipProgram.methods
      .previewQuote(notionalUsdc)
      .accounts(previewQuoteAccounts as any)
      .instruction();
    const {
      unitsConsumed: previewQuoteCu,
      value: previewQuote,
    } = await simulateViewWithBudget<any>(
      ctx,
      flagshipProgram,
      borrower.keypair,
      previewQuoteIx,
      "previewQuote"
    );

    const policyId = Keypair.generate().publicKey;
    const policyHeader = pda(
      [SEEDS.policy, policyId.toBuffer()],
      ctx.programs.kernel.programId
    );
    const productTerms = pda(
      [SEEDS.terms, policyId.toBuffer()],
      ctx.programs.flagshipAutocall.programId
    );
    const acceptSignature = await ctx.programs.flagshipAutocall.methods
      .acceptQuote({
        policyId,
        notionalUsdc,
        maxPremium: new BN(previewQuote.premium.toString()),
        minMaxLiability: new BN(previewQuote.maxLiability.toString()),
        minOfferedCouponBpsS6: new BN(
          previewQuote.offeredCouponBpsS6.toString()
        ),
        previewQuoteSlot: new BN(previewQuote.quoteSlot.toString()),
        maxQuoteSlotDelta: new BN(10_000),
        previewEntrySpyPriceS6: new BN(previewQuote.entrySpyPriceS6.toString()),
        previewEntryQqqPriceS6: new BN(previewQuote.entryQqqPriceS6.toString()),
        previewEntryIwmPriceS6: new BN(previewQuote.entryIwmPriceS6.toString()),
        maxEntryPriceDeviationBps: 10,
        previewExpiryTs: new BN(previewQuote.expiryTs.toString()),
        maxExpiryDeltaSecs: new BN(60),
      })
      .accounts({
        buyer: borrower.keypair.publicKey,
        policyHeader,
        productTerms,
        productAuthority: ctx.products.flagship.authority,
        usdcMint: ctx.usdcMint,
        buyerUsdc: borrower.usdc,
        vaultUsdc: ctx.pdas.vaultUsdc,
        treasuryUsdc: ctx.pdas.treasuryUsdc,
        vaultAuthority: ctx.pdas.vaultAuthority,
        protocolConfig: ctx.pdas.protocolConfig,
        vaultSigma: ctx.products.flagship.vaultSigma,
        regression: ctx.products.flagship.regression,
        pythSpy: new PublicKey(ctx.oracles["flagship-spy-entry"].pubkey),
        pythQqq: new PublicKey(ctx.oracles["flagship-qqq-entry"].pubkey),
        pythIwm: new PublicKey(ctx.oracles["flagship-iwm-entry"].pubkey),
        vaultState: ctx.pdas.vaultState,
        feeLedger: ctx.pdas.feeLedger,
        productRegistryEntry: ctx.products.flagship.productRegistryEntry,
        clock: SYSVAR_CLOCK_PUBKEY,
        kernelProgram: ctx.programs.kernel.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        autocallSchedule,
      } as any)
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
      ])
      .signers([borrower.keypair])
      .rpc();
    await ctx.provider.connection.confirmTransaction(
      acceptSignature,
      "confirmed"
    );

    const borrowerReceipt = receiptAccounts(
      ctx.programs.kernel.programId,
      policyHeader,
      borrower.keypair.publicKey
    );
    const lenderReceipt = receiptAccounts(
      ctx.programs.kernel.programId,
      policyHeader,
      lender.keypair.publicKey
    );
    const wrapTx = await buildCheckpointBatchTx(ctx, borrower.keypair, [
      ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
      kernelWrapPolicyReceiptIx(
        ctx.programs.kernel.programId,
        borrower.keypair.publicKey,
        policyHeader
      ),
    ]);
    const wrapCu = await simulateTransactionUnits(ctx, wrapTx, [
      borrower.keypair,
    ]);
    await sendSignedTransaction(ctx, wrapTx, [borrower.keypair]);

    const wrappedHeader = await ctx.programs.kernel.account.policyHeader.fetch(
      policyHeader
    );
    expect(wrappedHeader.owner.toBase58()).to.eq(
      borrowerReceipt.receiptAuthority.toBase58()
    );
    expect(
      (
        await getAccount(
          ctx.provider.connection,
          borrowerReceipt.holderReceiptToken,
          "confirmed"
        )
      ).amount
    ).to.eq(1n);

    const transferReceiptTx = await buildCheckpointBatchTx(ctx, borrower.keypair, [
      ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
      createAssociatedTokenAccountIx(
        borrower.keypair.publicKey,
        lenderReceipt.holderReceiptToken,
        lender.keypair.publicKey,
        borrowerReceipt.receiptMint
      ),
      splTokenTransferIx(
        borrowerReceipt.holderReceiptToken,
        lenderReceipt.holderReceiptToken,
        borrower.keypair.publicKey,
        1n
      ),
    ]);
    const transferReceiptCu = await simulateTransactionUnits(
      ctx,
      transferReceiptTx,
      [borrower.keypair]
    );
    await sendSignedTransaction(ctx, transferReceiptTx, [borrower.keypair]);

    expect(
      (
        await getAccount(
          ctx.provider.connection,
          borrowerReceipt.holderReceiptToken,
          "confirmed"
        )
      ).amount
    ).to.eq(0n);
    expect(
      (
        await getAccount(
          ctx.provider.connection,
          lenderReceipt.holderReceiptToken,
          "confirmed"
        )
      ).amount
    ).to.eq(1n);

    const previewLendingAccounts = {
      clock: SYSVAR_CLOCK_PUBKEY,
      policyHeader,
      productTerms,
      protocolConfig: ctx.pdas.protocolConfig,
      pythIwm: new PublicKey(ctx.oracles["flagship-iwm-upside"].pubkey),
      pythQqq: new PublicKey(ctx.oracles["flagship-qqq-upside"].pubkey),
      pythSpy: new PublicKey(ctx.oracles["flagship-spy-upside"].pubkey),
      regression: flagshipRegression,
      vaultSigma: ctx.products.flagship.vaultSigma,
    } as const;
    const buybackCheckpoint = await prepareMidlifeCheckpoint(
      ctx,
      flagshipProgram,
      lender.keypair,
      previewLendingAccounts
    );
    const buybackPreviewResult = await simulateCheckpointedLendingValue(
      ctx,
      flagshipProgram,
      lender.keypair,
      buybackCheckpoint.checkpoint.publicKey,
      {
        clock: SYSVAR_CLOCK_PUBKEY,
        policyHeader,
        productTerms,
      }
    );
    const expectedPayout = BigInt(
      buybackPreviewResult.preview.lendingValuePayoutUsdc
    );
    const lenderUsdcBefore = await getAccount(
      ctx.provider.connection,
      lender.usdc,
      "confirmed"
    );
    const vaultUsdcBefore = await getAccount(
      ctx.provider.connection,
      ctx.pdas.vaultUsdc,
      "confirmed"
    );

    const liquidationTx = await buildCheckpointBatchTx(ctx, lender.keypair, [
      ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
      kernelUnwrapPolicyReceiptIx(
        ctx.programs.kernel.programId,
        lender.keypair.publicKey,
        policyHeader
      ),
      flagshipBuybackFromCheckpointIx(
        flagshipProgram.programId,
        lender.keypair.publicKey,
        buybackCheckpoint.checkpoint.publicKey,
        {
          clock: SYSVAR_CLOCK_PUBKEY,
          kernelProgram: ctx.programs.kernel.programId,
          ownerUsdc: lender.usdc,
          policyHeader,
          productAuthority: ctx.products.flagship.authority,
          productRegistryEntry: ctx.products.flagship.productRegistryEntry,
          productTerms,
          protocolConfig: ctx.pdas.protocolConfig,
          usdcMint: ctx.usdcMint,
          vaultAuthority: ctx.pdas.vaultAuthority,
          vaultState: ctx.pdas.vaultState,
          vaultUsdc: ctx.pdas.vaultUsdc,
        }
      ),
    ]);
    const liquidationCu = await simulateTransactionUnits(ctx, liquidationTx, [
      lender.keypair,
    ]);
    await sendSignedTransaction(ctx, liquidationTx, [lender.keypair]);

    const lenderUsdcAfter = await getAccount(
      ctx.provider.connection,
      lender.usdc,
      "confirmed"
    );
    const vaultUsdcAfter = await getAccount(
      ctx.provider.connection,
      ctx.pdas.vaultUsdc,
      "confirmed"
    );
    expect(lenderUsdcAfter.amount - lenderUsdcBefore.amount).to.eq(
      expectedPayout
    );
    expect(vaultUsdcBefore.amount - vaultUsdcAfter.amount).to.eq(
      expectedPayout
    );
    expect(
      (
        await getAccount(
          ctx.provider.connection,
          lenderReceipt.holderReceiptToken,
          "confirmed"
        )
      ).amount
    ).to.eq(0n);
    const settledHeader = await ctx.programs.kernel.account.policyHeader.fetch(
      policyHeader
    );
    const settledTerms =
      await flagshipProgram.account.flagshipAutocallTerms.fetch(productTerms);
    expect(settledHeader.owner.toBase58()).to.eq(
      lender.keypair.publicKey.toBase58()
    );
    expect(settledHeader.status.settled).to.not.be.undefined;
    expect(settledTerms.status.settled).to.not.be.undefined;
    expect(settledTerms.settledPayoutUsdc.toString()).to.eq(
      expectedPayout.toString()
    );
    expect(
      await ctx.provider.connection.getAccountInfo(
        borrowerReceipt.policyReceipt,
        "confirmed"
      )
    ).to.eq(null);
    expect(
      await ctx.provider.connection.getAccountInfo(
        buybackCheckpoint.checkpoint.publicKey,
        "confirmed"
      )
    ).to.eq(null);

    console.log(
      `flagship collateral_liquidation preview_quote_cu=${previewQuoteCu} wrap_cu=${wrapCu} transfer_receipt_cu=${transferReceiptCu} checkpoint_prepare_chunk=${buybackCheckpoint.prepareChunkSize} checkpoint_prepare_max_cu=${buybackCheckpoint.unitsConsumed} checkpoint_advance_chunks=${buybackCheckpoint.advanceChunkSizes.join(",") || "none"} checkpoint_advance_cu=${buybackCheckpoint.advanceUnitsConsumed.join(",") || "none"} liquidation_cu=${liquidationCu} nav=${buybackPreviewResult.preview.navS6} lending_value=${buybackPreviewResult.preview.lendingValueS6} payout_usdc=${expectedPayout.toString()} lender_usdc_delta=${(lenderUsdcAfter.amount - lenderUsdcBefore.amount).toString()}`
    );

    expect(wrapCu).to.be.lessThan(1_400_000);
    expect(transferReceiptCu).to.be.lessThan(1_400_000);
    expect(buybackCheckpoint.unitsConsumed).to.be.at.most(
      MIDLIFE_CHECKPOINT_TARGET_UNITS
    );
    expect(buybackPreviewResult.unitsConsumed).to.be.at.most(
      MIDLIFE_CHECKPOINT_TARGET_UNITS
    );
    expect(liquidationCu).to.be.lessThan(1_400_000);
    expect(expectedPayout > 0n).to.eq(true);
  });
});
