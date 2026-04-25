import { BN } from "@coral-xyz/anchor";
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
import { assert, expect } from "chai";
import { execFileSync } from "child_process";
import { createHash } from "crypto";

import { setupFullProtocol, TestContext } from "./setup";
import { TOKEN_PROGRAM_ID } from "../kernel/token_harness";

const SEEDS = {
  policy: Buffer.from("policy"),
  terms: Buffer.from("terms"),
  reducedOperators: Buffer.from("reduced_operators"),
  midlifeMatrices: Buffer.from("midlife_matrices"),
} as const;
const SOL_TEST_SIGMA_S6 = 800_000;
const SOL_TEST_STEP_DAYS_S6 = 1_000_000;
const SOL_MIDLIFE_MATRIX_INPUTS_HASH_DOMAIN =
  "halcyon:sol-autocall:midlife-matrix-inputs:v1";
const SOL_MIDLIFE_MATRIX_VALUES_HASH_DOMAIN =
  "halcyon:sol-autocall:midlife-matrix-values:v1";
const SOL_MIDLIFE_CURRENT_ENGINE_VERSION = 1;
const SOL_MIDLIFE_OBSERVATION_COUNT = 8;
const SOL_MIDLIFE_MATURITY_DAYS = 8;
const SOL_MIDLIFE_OBSERVATION_INTERVAL_DAYS = 1;
const SOL_MIDLIFE_SECONDS_PER_DAY = 1;
const SOL_MIDLIFE_NO_AUTOCALL_FIRST_N_OBS = 1;
const SOL_MIDLIFE_KI_BARRIER_BPS = 7_000;
const SOL_MIDLIFE_KNOCK_IN_LOG_6 = -356_675;
const SOL_MIDLIFE_AUTOCALL_LOG_6 = 24_693;
const SOL_MIDLIFE_TRAINING_ALPHA_S6 = 13_040_000;
const SOL_MIDLIFE_TRAINING_BETA_S6 = 1_520_000;
const SOL_MIDLIFE_TRAINING_REFERENCE_STEP_DAYS = 2;
const SOL_MIDLIFE_TRAINING_NO_AUTOCALL_FIRST_N_OBS = 1;
const LONG_INTEGRATION_FRESHNESS_CAP_SECS = 86_400;

function pda(seeds: Buffer[], programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, programId)[0];
}

function asBigInt(value: unknown): bigint {
  if (typeof value === "bigint") return value;
  if (typeof value === "number") return BigInt(value);
  if (BN.isBN(value)) return BigInt(value.toString());
  if (value && typeof (value as { toString(): string }).toString === "function") {
    return BigInt((value as { toString(): string }).toString());
  }
  throw new Error(`cannot convert value to bigint: ${String(value)}`);
}

function u8(value: number): Buffer {
  return Buffer.from([value & 0xff]);
}

function u16(value: number): Buffer {
  const out = Buffer.alloc(2);
  out.writeUInt16LE(value);
  return out;
}

function u32(value: number): Buffer {
  const out = Buffer.alloc(4);
  out.writeUInt32LE(value);
  return out;
}

function u64(value: unknown): Buffer {
  const out = Buffer.alloc(8);
  out.writeBigUInt64LE(asBigInt(value));
  return out;
}

function i64(value: unknown): Buffer {
  const out = Buffer.alloc(8);
  out.writeBigInt64LE(asBigInt(value));
  return out;
}

function sha256(chunks: Buffer[]): Buffer {
  return createHash("sha256").update(Buffer.concat(chunks)).digest();
}

function solMidlifeConstructionInputsHash(account: any): Buffer {
  const stepCount = Number(account.uploadedStepCount);
  const chunks: Buffer[] = [
    Buffer.from(SOL_MIDLIFE_MATRIX_INPUTS_HASH_DOMAIN),
    u8(Number(account.version)),
    u16(SOL_MIDLIFE_CURRENT_ENGINE_VERSION),
    i64(account.sigmaAnnS6),
    u16(Number(account.nStates)),
    u16(Number(account.cosTerms)),
    u16(9),
    u16(81),
    u16(SOL_MIDLIFE_OBSERVATION_COUNT),
    u32(SOL_MIDLIFE_MATURITY_DAYS),
    u32(SOL_MIDLIFE_OBSERVATION_INTERVAL_DAYS),
    i64(SOL_MIDLIFE_SECONDS_PER_DAY),
    u8(SOL_MIDLIFE_NO_AUTOCALL_FIRST_N_OBS),
    u16(SOL_MIDLIFE_KI_BARRIER_BPS),
    i64(SOL_MIDLIFE_KNOCK_IN_LOG_6),
    i64(SOL_MIDLIFE_AUTOCALL_LOG_6),
    i64(SOL_MIDLIFE_TRAINING_ALPHA_S6),
    i64(SOL_MIDLIFE_TRAINING_BETA_S6),
    i64(SOL_MIDLIFE_TRAINING_REFERENCE_STEP_DAYS),
    u16(SOL_MIDLIFE_TRAINING_NO_AUTOCALL_FIRST_N_OBS),
    u64(account.sourceVaultSigmaSlot),
    u64(account.sourceRegimeSignalSlot),
    u16(stepCount),
  ];
  for (let idx = 0; idx < stepCount; idx += 1) {
    chunks.push(i64(account.stepDaysS6[idx]));
  }
  return sha256(chunks);
}

function solMidlifeMatrixValuesHash(
  account: any,
  constructionInputsSha256: Buffer
): Buffer {
  const stepCount = Number(account.uploadedStepCount);
  const chunks: Buffer[] = [
    Buffer.from(SOL_MIDLIFE_MATRIX_VALUES_HASH_DOMAIN),
    constructionInputsSha256,
    u16(stepCount),
  ];
  for (let idx = 0; idx < stepCount; idx += 1) {
    chunks.push(i64(account.stepDaysS6[idx]));
    chunks.push(u16(Number(account.uploadedLens[idx])));
  }
  chunks.push(u32(account.matrices.length));
  for (const value of account.matrices) {
    chunks.push(i64(value));
  }
  return sha256(chunks);
}

function expectHash(actual: number[] | Buffer, expected: Buffer) {
  expect(Buffer.from(actual).toString("hex")).to.eq(expected.toString("hex"));
}

function lastProgramReturnData(
  logs: string[] | null | undefined,
  programId: PublicKey
): Buffer | null {
  const returnPrefix = `Program return: ${programId.toBase58()} `;
  const returnLog = logs?.filter((log) => log.startsWith(returnPrefix)).at(-1);
  return returnLog
    ? Buffer.from(returnLog.slice(returnPrefix.length), "base64")
    : null;
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

async function simulateViewWithBudget<T>(
  ctx: TestContext,
  program: any,
  payer: Keypair,
  ix: TransactionInstruction,
  instructionName: string
): Promise<{ unitsConsumed: number; value: T }> {
  const recentBlockhash = await ctx.provider.connection.getLatestBlockhash(
    "confirmed"
  );
  const tx = new Transaction({
    feePayer: payer.publicKey,
    recentBlockhash: recentBlockhash.blockhash,
  }).add(ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }), ix);
  const result = await simulateLegacyTransaction(ctx, tx, [payer]);
  if (result.value.err) {
    throw new Error(
      `simulation failed: ${JSON.stringify(result.value.err)}\n${
        result.value.logs?.join("\n") ?? ""
      }`
    );
  }
  const returnData = lastProgramReturnData(
    result.value.logs,
    program.programId
  );
  if (!returnData) {
    throw new Error(`missing return log for ${program.programId.toBase58()}`);
  }
  const idlIx = program.idl.instructions.find(
    (candidate: any) => candidate.name === instructionName
  );
  if (!idlIx?.returns) {
    throw new Error(`missing IDL return type for ${instructionName}`);
  }
  const { IdlCoder } = require("@coral-xyz/anchor/dist/cjs/coder/borsh/idl");
  const coder = IdlCoder.fieldLayout(
    { type: idlIx.returns },
    program.idl.types
  );
  return {
    unitsConsumed: result.value.unitsConsumed ?? 0,
    value: coder.decode(returnData) as T,
  };
}

async function confirmSignature(
  ctx: TestContext,
  signature: string,
  timeoutMs = 120_000
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const { value } = await ctx.provider.connection.getSignatureStatuses(
      [signature],
      { searchTransactionHistory: true }
    );
    const status = value[0];
    if (status?.err) {
      throw new Error(
        `transaction ${signature} failed: ${JSON.stringify(status.err)}`
      );
    }
    if (
      status?.confirmationStatus === "confirmed" ||
      status?.confirmationStatus === "finalized"
    ) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(
    `transaction ${signature} was not confirmed within ${timeoutMs}ms`
  );
}

async function setSolSigmaFloorForReducedOps(ctx: TestContext): Promise<void> {
  const signature = await ctx.programs.kernel.methods
    .setProtocolConfig({
      utilizationCapBps: null,
      sigmaStalenessCapSecs: null,
      regimeStalenessCapSecs: null,
      regressionStalenessCapSecs: null,
      pythQuoteStalenessCapSecs: null,
      pythSettleStalenessCapSecs: null,
      quoteTtlSecs: null,
      ewmaRateLimitSecs: null,
      ilEwmaRateLimitSecs: null,
      solAutocallEwmaRateLimitSecs: null,
      seniorCooldownSecs: null,
      sigmaFloorAnnualisedS6: null,
      ilSigmaFloorAnnualisedS6: null,
      solAutocallSigmaFloorAnnualisedS6: new BN(SOL_TEST_SIGMA_S6),
      flagshipSigmaFloorAnnualisedS6: null,
      sigmaCeilingAnnualisedS6: null,
      k12CorrectionSha256: null,
      dailyKiCorrectionSha256: null,
      podDeimTableSha256: null,
      premiumSplitsBps: null,
      solAutocallQuoteConfigBps: null,
      treasuryDestination: null,
      hedgeMaxSlippageBpsCap: null,
      hedgeDefundDestination: null,
    })
    .accounts({
      admin: ctx.admin.publicKey,
      protocolConfig: ctx.pdas.protocolConfig,
    } as any)
    .rpc();
  await confirmSignature(ctx, signature);
}

async function extendStaticFixtureFreshnessCaps(ctx: TestContext): Promise<void> {
  const cap = new BN(LONG_INTEGRATION_FRESHNESS_CAP_SECS);
  const signature = await ctx.programs.kernel.methods
    .setProtocolConfig({
      utilizationCapBps: null,
      sigmaStalenessCapSecs: cap,
      regimeStalenessCapSecs: cap,
      regressionStalenessCapSecs: cap,
      pythQuoteStalenessCapSecs: cap,
      pythSettleStalenessCapSecs: cap,
      quoteTtlSecs: null,
      ewmaRateLimitSecs: null,
      ilEwmaRateLimitSecs: null,
      solAutocallEwmaRateLimitSecs: null,
      seniorCooldownSecs: null,
      sigmaFloorAnnualisedS6: null,
      ilSigmaFloorAnnualisedS6: null,
      solAutocallSigmaFloorAnnualisedS6: null,
      flagshipSigmaFloorAnnualisedS6: null,
      sigmaCeilingAnnualisedS6: null,
      k12CorrectionSha256: null,
      dailyKiCorrectionSha256: null,
      podDeimTableSha256: null,
      premiumSplitsBps: null,
      solAutocallQuoteConfigBps: null,
      treasuryDestination: null,
      hedgeMaxSlippageBpsCap: null,
      hedgeDefundDestination: null,
    })
    .accounts({
      admin: ctx.admin.publicKey,
      protocolConfig: ctx.pdas.protocolConfig,
    } as any)
    .rpc();
  await confirmSignature(ctx, signature);
}

function loadReducedOps(): {
  fair_coupon_bps: number;
  p_red_v: number[];
  p_red_u: number[];
} {
  const stdout = execFileSync(
    "cargo",
    [
      "run",
      "-q",
      "-p",
      "halcyon_sol_autocall_quote",
      "--bin",
      "sol_reduced_ops",
      "--",
      SOL_TEST_SIGMA_S6.toString(),
    ],
    { cwd: process.cwd(), encoding: "utf8" }
  );
  return JSON.parse(stdout);
}

async function uploadSolReducedOps(ctx: TestContext): Promise<PublicKey> {
  const reduced = loadReducedOps();
  assert(
    reduced.fair_coupon_bps >= 50,
    `SOL reduced operators must be quoteable; fair_coupon_bps=${reduced.fair_coupon_bps}`
  );
  const reducedOperators = pda(
    [SEEDS.reducedOperators],
    ctx.programs.solAutocall.programId
  );
  const chunkSize = 48;
  for (const [sideName, values] of [
    ["v", reduced.p_red_v],
    ["u", reduced.p_red_u],
  ] as const) {
    for (let start = 0; start < values.length; start += chunkSize) {
      const chunk = values
        .slice(start, Math.min(start + chunkSize, values.length))
        .map((value) => new BN(value));
      const signature = await ctx.programs.solAutocall.methods
        .writeReducedOperators({
          beginUpload: sideName === "v" && start === 0,
          side: { [sideName]: {} },
          start,
          values: chunk,
        })
        .accounts({
          keeper: ctx.keepers.regime.publicKey,
          protocolConfig: ctx.pdas.protocolConfig,
          keeperRegistry: ctx.pdas.keeperRegistry,
          vaultSigma: ctx.products.sol.vaultSigma,
          regimeSignal: ctx.products.sol.regimeSignal,
          reducedOperators,
          systemProgram: SystemProgram.programId,
        } as any)
        .signers([ctx.keepers.regime])
        .rpc();
      await confirmSignature(ctx, signature);
    }
  }
  return reducedOperators;
}

function loadMidlifeMatrices(): {
  sigma_ann_s6: number;
  n_states: number;
  cos_terms: number;
  steps: { step_days_s6: number; values: number[] }[];
} {
  const stdout = execFileSync(
    "cargo",
    [
      "run",
      "-q",
      "-p",
      "halcyon_sol_autocall_quote",
      "--bin",
      "sol_midlife_matrices",
      "--",
      SOL_TEST_SIGMA_S6.toString(),
      SOL_TEST_STEP_DAYS_S6.toString(),
    ],
    { cwd: process.cwd(), encoding: "utf8" }
  );
  return JSON.parse(stdout);
}

async function uploadSolMidlifeMatrices(ctx: TestContext): Promise<PublicKey> {
  const upload = loadMidlifeMatrices();
  expect(upload.sigma_ann_s6).to.eq(SOL_TEST_SIGMA_S6);
  expect(upload.n_states).to.eq(9);
  expect(upload.cos_terms).to.eq(13);
  const midlifeMatrices = pda(
    [SEEDS.midlifeMatrices],
    ctx.programs.solAutocall.programId
  );
  const chunkSize = 48;
  for (let stepIndex = 0; stepIndex < upload.steps.length; stepIndex += 1) {
    const step = upload.steps[stepIndex];
    expect(step.values.length).to.eq(81);
    for (let start = 0; start < step.values.length; start += chunkSize) {
      const chunk = step.values
        .slice(start, Math.min(start + chunkSize, step.values.length))
        .map((value) => new BN(value));
      const signature = await ctx.programs.solAutocall.methods
        .writeMidlifeMatrices({
          beginUpload: stepIndex === 0 && start === 0,
          stepIndex,
          stepDaysS6: new BN(step.step_days_s6),
          start,
          values: chunk,
        })
        .accounts({
          keeper: ctx.keepers.regime.publicKey,
          protocolConfig: ctx.pdas.protocolConfig,
          keeperRegistry: ctx.pdas.keeperRegistry,
          vaultSigma: ctx.products.sol.vaultSigma,
          regimeSignal: ctx.products.sol.regimeSignal,
          midlifeMatrices,
          systemProgram: SystemProgram.programId,
        } as any)
        .signers([ctx.keepers.regime])
        .rpc();
      await confirmSignature(ctx, signature);
    }
  }
  const matrixAccount =
    await ctx.programs.solAutocall.account.solAutocallMidlifeMatrices.fetch(
      midlifeMatrices
    );
  const replayedValues = upload.steps.flatMap((step) => step.values);
  expect(
    matrixAccount.matrices.map((value: any) => Number(asBigInt(value)))
  ).to.deep.eq(replayedValues);
  for (let idx = 0; idx < upload.steps.length; idx += 1) {
    expect(Number(asBigInt(matrixAccount.stepDaysS6[idx]))).to.eq(
      upload.steps[idx].step_days_s6
    );
    expect(Number(matrixAccount.uploadedLens[idx])).to.eq(
      upload.steps[idx].values.length
    );
  }
  const expectedInputsHash = solMidlifeConstructionInputsHash(matrixAccount);
  const expectedValuesHash = solMidlifeMatrixValuesHash(
    matrixAccount,
    expectedInputsHash
  );
  expectHash(matrixAccount.constructionInputsSha256, expectedInputsHash);
  expectHash(matrixAccount.matrixValuesSha256, expectedValuesHash);
  return midlifeMatrices;
}

describe("SOL and IL midlife pricers on-chain", function () {
  this.timeout(1_000_000);

  let ctx: TestContext;

  before(async () => {
    ctx = await setupFullProtocol();
    await extendStaticFixtureFreshnessCaps(ctx);
  });

  it("issues IL protection and previews shifted NIG midlife value", async () => {
    const buyer = ctx.buyers[0];
    const notionalUsdc = new BN(5_000_000_000);
    const previewQuoteIx = await ctx.programs.ilProtection.methods
      .previewQuote(notionalUsdc)
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
    const { value: quote } = await simulateViewWithBudget<any>(
      ctx,
      ctx.programs.ilProtection,
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
      ctx.programs.ilProtection.programId
    );

    const acceptSignature = await ctx.programs.ilProtection.methods
      .acceptQuote({
        policyId,
        insuredNotionalUsdc: notionalUsdc,
        maxPremium: new BN(quote.premium.toString()),
        minMaxLiability: new BN(quote.maxLiability.toString()),
        previewQuoteSlot: new BN(quote.quoteSlot.toString()),
        maxQuoteSlotDelta: new BN(10_000),
        previewEntrySolPriceS6: new BN(quote.entrySolPriceS6.toString()),
        previewEntryUsdcPriceS6: new BN(quote.entryUsdcPriceS6.toString()),
        maxEntryPriceDeviationBps: 10,
        previewExpiryTs: new BN(quote.expiryTs.toString()),
        maxExpiryDeltaSecs: new BN(60),
      })
      .accounts({
        buyer: buyer.keypair.publicKey,
        policyHeader,
        productTerms,
        productAuthority: ctx.products.il.authority,
        usdcMint: ctx.usdcMint,
        buyerUsdc: buyer.usdc,
        vaultUsdc: ctx.pdas.vaultUsdc,
        treasuryUsdc: ctx.pdas.treasuryUsdc,
        vaultAuthority: ctx.pdas.vaultAuthority,
        protocolConfig: ctx.pdas.protocolConfig,
        vaultSigma: ctx.products.il.vaultSigma,
        regimeSignal: ctx.products.il.regimeSignal,
        pythSol: new PublicKey(ctx.oracles["il-sol-entry"].pubkey),
        pythUsdc: new PublicKey(ctx.oracles["il-usdc-entry"].pubkey),
        vaultState: ctx.pdas.vaultState,
        feeLedger: ctx.pdas.feeLedger,
        productRegistryEntry: ctx.products.il.productRegistryEntry,
        clock: SYSVAR_CLOCK_PUBKEY,
        kernelProgram: ctx.programs.kernel.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      } as any)
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
      ])
      .signers([buyer.keypair])
      .rpc();
    await confirmSignature(ctx, acceptSignature);

    const previewLendingIx = await ctx.programs.ilProtection.methods
      .previewLendingValue()
      .accounts({
        protocolConfig: ctx.pdas.protocolConfig,
        vaultSigma: ctx.products.il.vaultSigma,
        regimeSignal: ctx.products.il.regimeSignal,
        policyHeader,
        productTerms,
        pythSol: new PublicKey(ctx.oracles["il-sol-crash"].pubkey),
        pythUsdc: new PublicKey(ctx.oracles["il-usdc-entry"].pubkey),
        clock: SYSVAR_CLOCK_PUBKEY,
      } as any)
      .instruction();
    const { unitsConsumed, value: preview } = await simulateViewWithBudget<any>(
      ctx,
      ctx.programs.ilProtection,
      buyer.keypair,
      previewLendingIx,
      "previewLendingValue"
    );

    expect(new BN(preview.navS6.toString()).gt(new BN(0))).to.eq(true);
    expect(new BN(preview.intrinsicPayoutUsdc.toString()).gt(new BN(0))).to.eq(
      true
    );
    expect(
      new BN(preview.navPayoutUsdc.toString()).gte(
        new BN(preview.intrinsicPayoutUsdc.toString())
      )
    ).to.eq(true);
    expect(
      new BN(preview.lendingValuePayoutUsdc.toString()).lte(
        new BN(preview.intrinsicPayoutUsdc.toString()).muln(80).divn(100)
      )
    ).to.eq(true);
    expect(unitsConsumed).to.be.lessThan(1_400_000);
  });

  it("issues SOL autocall and previews Markov midlife value", async () => {
    await setSolSigmaFloorForReducedOps(ctx);
    const reducedOperators = await uploadSolReducedOps(ctx);
    const midlifeMatrices = await uploadSolMidlifeMatrices(ctx);
    const buyer = ctx.buyers[1];
    const notionalUsdc = new BN(5_000_000_000);

    const previewQuoteIx = await ctx.programs.solAutocall.methods
      .previewQuote(notionalUsdc)
      .accounts({
        clock: SYSVAR_CLOCK_PUBKEY,
        productRegistryEntry: ctx.products.sol.productRegistryEntry,
        protocolConfig: ctx.pdas.protocolConfig,
        reducedOperators,
        pythSol: new PublicKey(ctx.oracles["sol-entry"].pubkey),
        regimeSignal: ctx.products.sol.regimeSignal,
        vaultSigma: ctx.products.sol.vaultSigma,
      } as any)
      .instruction();
    const { value: quote } = await simulateViewWithBudget<any>(
      ctx,
      ctx.programs.solAutocall,
      buyer.keypair,
      previewQuoteIx,
      "previewQuote"
    );
    expect(new BN(quote.maxLiability.toString()).gt(new BN(0))).to.eq(true);
    expect(new BN(quote.offeredCouponBpsS6.toString()).gt(new BN(0))).to.eq(
      true
    );

    const policyId = Keypair.generate().publicKey;
    const policyHeader = pda(
      [SEEDS.policy, policyId.toBuffer()],
      ctx.programs.kernel.programId
    );
    const productTerms = pda(
      [SEEDS.terms, policyId.toBuffer()],
      ctx.programs.solAutocall.programId
    );
    const acceptSignature = await ctx.programs.solAutocall.methods
      .acceptQuote({
        policyId,
        notionalUsdc,
        maxPremium: new BN(quote.premium.toString()),
        minMaxLiability: new BN(quote.maxLiability.toString()),
        minOfferedCouponBpsS6: new BN(quote.offeredCouponBpsS6.toString()),
        previewQuoteSlot: new BN(quote.quoteSlot.toString()),
        maxQuoteSlotDelta: new BN(10_000),
        previewEntryPriceS6: new BN(quote.entryPriceS6.toString()),
        maxEntryPriceDeviationBps: 10,
        previewExpiryTs: new BN(quote.expiryTs.toString()),
        maxExpiryDeltaSecs: new BN(60),
      })
      .accounts({
        buyer: buyer.keypair.publicKey,
        policyHeader,
        productTerms,
        productAuthority: ctx.products.sol.authority,
        usdcMint: ctx.usdcMint,
        buyerUsdc: buyer.usdc,
        vaultUsdc: ctx.pdas.vaultUsdc,
        treasuryUsdc: ctx.pdas.treasuryUsdc,
        vaultAuthority: ctx.pdas.vaultAuthority,
        protocolConfig: ctx.pdas.protocolConfig,
        vaultSigma: ctx.products.sol.vaultSigma,
        regimeSignal: ctx.products.sol.regimeSignal,
        reducedOperators,
        pythSol: new PublicKey(ctx.oracles["sol-entry"].pubkey),
        vaultState: ctx.pdas.vaultState,
        feeLedger: ctx.pdas.feeLedger,
        productRegistryEntry: ctx.products.sol.productRegistryEntry,
        clock: SYSVAR_CLOCK_PUBKEY,
        kernelProgram: ctx.programs.kernel.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      } as any)
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }),
      ])
      .signers([buyer.keypair])
      .rpc();
    await confirmSignature(ctx, acceptSignature);

    const previewLendingIx = await ctx.programs.solAutocall.methods
      .previewLendingValue()
      .accounts({
        protocolConfig: ctx.pdas.protocolConfig,
        vaultSigma: ctx.products.sol.vaultSigma,
        regimeSignal: ctx.products.sol.regimeSignal,
        policyHeader,
        productTerms,
        midlifeMatrices,
        pythSol: new PublicKey(ctx.oracles["sol-autocall"].pubkey),
        clock: SYSVAR_CLOCK_PUBKEY,
      } as any)
      .instruction();
    const { unitsConsumed, value: preview } = await simulateViewWithBudget<any>(
      ctx,
      ctx.programs.solAutocall,
      buyer.keypair,
      previewLendingIx,
      "previewLendingValue"
    );

    expect(new BN(preview.navS6.toString()).gt(new BN(0))).to.eq(true);
    expect(
      new BN(preview.lendingValueS6.toString()).lte(
        new BN(preview.navS6.toString())
      )
    ).to.eq(true);
    expect(new BN(preview.sigmaPricingS6.toString()).toString()).to.eq(
      SOL_TEST_SIGMA_S6.toString()
    );
    expect(preview.modelStates).to.eq(9);
    expect(unitsConsumed).to.be.lessThan(1_400_000);
  });
});
