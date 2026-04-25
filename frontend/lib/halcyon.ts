import { Buffer } from "buffer";
import { BorshAccountsCoder, BorshCoder, BorshInstructionCoder, BN, type Idl } from "@coral-xyz/anchor";
import {
  AddressLookupTableAccount,
  Connection,
  ComputeBudgetProgram,
  Keypair,
  PublicKey,
  SystemProgram,
  SYSVAR_CLOCK_PUBKEY,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
  type Commitment,
  type GetProgramAccountsFilter,
} from "@solana/web3.js";
import bs58 from "bs58";

import flagshipIdlJson from "../../target/idl/halcyon_flagship_autocall.json";
import ilIdlJson from "../../target/idl/halcyon_il_protection.json";
import kernelIdlJson from "../../target/idl/halcyon_kernel.json";
import lendingIdlJson from "../../target/idl/halcyon_lending_consumer.json";
import solIdlJson from "../../target/idl/halcyon_sol_autocall.json";
import { enumTag, field, toNumber, toStringValue } from "@/lib/format";
import type { ClusterConfig, ProductKind } from "@/lib/types";

function isValidRuntimeConfigValue(key: keyof ClusterConfig, value: string): boolean {
  if (key === "rpcUrl") {
    try {
      const url = new URL(value);
      return url.protocol === "http:" || url.protocol === "https:";
    } catch {
      return false;
    }
  }
  try {
    new PublicKey(value);
    return true;
  } catch {
    return false;
  }
}

const flagshipIdl = flagshipIdlJson as Idl;
const ilIdl = ilIdlJson as Idl;
const kernelIdl = kernelIdlJson as Idl;
const lendingIdl = lendingIdlJson as Idl;
const solIdl = solIdlJson as Idl;

const coders = {
  flagship: new BorshCoder(flagshipIdl),
  il: new BorshCoder(ilIdl),
  kernel: new BorshCoder(kernelIdl),
  lending: new BorshCoder(lendingIdl),
  sol: new BorshCoder(solIdl),
};

const instructionCoders = {
  kernel: new BorshInstructionCoder(kernelIdl),
  flagship: new BorshInstructionCoder(flagshipIdl),
  il: new BorshInstructionCoder(ilIdl),
  lending: new BorshInstructionCoder(lendingIdl),
  sol: new BorshInstructionCoder(solIdl),
};

const accountCoders = {
  flagship: new BorshAccountsCoder(flagshipIdl),
  il: new BorshAccountsCoder(ilIdl),
  kernel: new BorshAccountsCoder(kernelIdl),
  lending: new BorshAccountsCoder(lendingIdl),
  sol: new BorshAccountsCoder(solIdl),
};

const SEEDS = {
  protocolConfig: Buffer.from("protocol_config"),
  productRegistry: Buffer.from("product_registry"),
  vaultState: Buffer.from("vault_state"),
  terms: Buffer.from("terms"),
  policy: Buffer.from("policy"),
  vaultAuthority: Buffer.from("vault_authority"),
  productAuthority: Buffer.from("product_authority"),
  vaultUsdc: Buffer.from("vault_usdc"),
  treasuryUsdc: Buffer.from("treasury_usdc"),
  feeLedger: Buffer.from("fee_ledger"),
  vaultSigma: Buffer.from("vault_sigma"),
  regimeSignal: Buffer.from("regime_signal"),
  regression: Buffer.from("regression"),
  autocallSchedule: Buffer.from("autocall_schedule"),
  reducedOperators: Buffer.from("reduced_operators"),
  altRegistry: Buffer.from("alt_registry"),
  couponVault: Buffer.from("coupon_vault"),
  keeperRegistry: Buffer.from("keeper_registry"),
  hedgeSleeve: Buffer.from("hedge_sleeve"),
  policyReceipt: Buffer.from("policy_receipt"),
  policyReceiptMint: Buffer.from("policy_receipt_mint"),
  policyReceiptAuthority: Buffer.from("policy_receipt_authority"),
  retailRedemption: Buffer.from("retail_redemption"),
};

const CONFIRMED: Commitment = "confirmed";
const FRONTEND_COMPUTE_UNIT_LIMIT = 1_400_000;
const MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE = 20_706;
const MIDLIFE_FINAL_COUPON_INDEX = 18;
const MIDLIFE_CHECKPOINT_MAX_CHUNK_SIZE = 18;
const MIDLIFE_CHECKPOINT_TARGET_UNITS = 1_280_000;

function withComputeBudget(instructions: TransactionInstruction[]) {
  return [
    ComputeBudgetProgram.setComputeUnitLimit({ units: FRONTEND_COMPUTE_UNIT_LIMIT }),
    ...instructions,
  ];
}
const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
);
const MEMO_PROGRAM_ID = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

function getAssociatedTokenAddressSync(mint: PublicKey, owner: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  )[0];
}

export interface ProtocolContext {
  kernelProgramId: PublicKey;
  productProgramId: PublicKey;
  protocolConfigAddress: PublicKey;
  protocolConfig: Record<string, unknown>;
  productRegistryAddress: PublicKey;
  productRegistry: Record<string, unknown>;
  usdcMint: PublicKey;
}

export interface ProductPreviewResult {
  data: Record<string, unknown>;
  protocolContext: ProtocolContext;
}

export type CheckpointTransactionSender = (
  transaction: VersionedTransaction,
  signers: Keypair[],
) => Promise<string>;

export interface CheckpointedLendingValueExecution {
  preview: Record<string, unknown>;
  checkpoint: PublicKey;
  signatures: string[];
  prepareChunkSize: number;
  advanceChunkSizes: number[];
  transactionUnitsConsumed: number[];
  maxUnitsConsumed: number;
}

export interface PortfolioEntry {
  policyAddress: string;
  productKind: ProductKind;
  owner: string;
  status: string;
  notional: number;
  premiumPaid: number;
  maxLiability: number;
  issuedAt: number;
  expiryTs: number;
  productTermsAddress: string;
  details: Record<string, string>;
}

export interface VaultOverview {
  protocolConfig: Record<string, unknown>;
  vaultState: Record<string, unknown>;
  feeLedger: Record<string, unknown> | null;
  keeperRegistry: Record<string, unknown> | null;
  productSummaries: Array<{
    kind: ProductKind;
    registry: Record<string, unknown>;
    activePolicyCount: number;
    settledPolicyCount: number;
    couponVaultBalance: number | null;
    hedgeReserve: number | null;
  }>;
}

export function programIdForKind(kind: ProductKind, config: ClusterConfig) {
  switch (kind) {
    case "flagship":
      return new PublicKey(config.flagshipProgramId);
    case "ilProtection":
      return new PublicKey(config.ilProtectionProgramId);
    case "solAutocall":
      return new PublicKey(config.solAutocallProgramId);
  }
}

export function kernelProgramId(config: ClusterConfig) {
  return new PublicKey(config.kernelProgramId);
}

export function lendingConsumerProgramId(config: ClusterConfig) {
  return new PublicKey(config.lendingConsumerProgramId);
}

export function feedAccountsForKind(kind: ProductKind, config: ClusterConfig) {
  switch (kind) {
    case "flagship":
      return {
        pythSpy: new PublicKey(config.pythSpy),
        pythQqq: new PublicKey(config.pythQqq),
        pythIwm: new PublicKey(config.pythIwm),
      };
    case "ilProtection":
      return {
        pythSol: new PublicKey(config.pythSol),
        pythUsdc: new PublicKey(config.pythUsdc),
      };
    case "solAutocall":
      return {
        pythSol: new PublicKey(config.pythSol),
      };
  }
}

export function missingFieldsForKind(kind: ProductKind, config: ClusterConfig) {
  const required = [
    { key: "rpcUrl", label: "RPC URL" },
    { key: "kernelProgramId", label: "Kernel program" },
  ] as Array<{ key: keyof ClusterConfig; label: string }>;

  if (kind === "flagship") {
    required.push(
      { key: "flagshipProgramId", label: "Flagship program" },
      { key: "pythSpy", label: "Pyth SPY account" },
      { key: "pythQqq", label: "Pyth QQQ account" },
      { key: "pythIwm", label: "Pyth IWM account" },
    );
  } else if (kind === "ilProtection") {
    required.push(
      { key: "ilProtectionProgramId", label: "IL Protection program" },
      { key: "pythSol", label: "Pyth SOL account" },
      { key: "pythUsdc", label: "Pyth USDC account" },
    );
  } else {
    required.push(
      { key: "solAutocallProgramId", label: "SOL Autocall program" },
      { key: "pythSol", label: "Pyth SOL account" },
    );
  }

  return required.filter(({ key }) => {
    const value = config[key];
    return !value?.trim() || !isValidRuntimeConfigValue(key, value);
  });
}

function productAuthority(productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.productAuthority], productProgramId)[0];
}

function protocolConfigAddress(kernelProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.protocolConfig], kernelProgramId)[0];
}

function productRegistryAddress(kernelProgramId: PublicKey, productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [SEEDS.productRegistry, productProgramId.toBuffer()],
    kernelProgramId,
  )[0];
}

function vaultSigmaAddress(kernelProgramId: PublicKey, productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.vaultSigma, productProgramId.toBuffer()], kernelProgramId)[0];
}

function regimeSignalAddress(kernelProgramId: PublicKey, productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [SEEDS.regimeSignal, productProgramId.toBuffer()],
    kernelProgramId,
  )[0];
}

function regressionAddress(kernelProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.regression], kernelProgramId)[0];
}

function autocallScheduleAddress(kernelProgramId: PublicKey, productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [SEEDS.autocallSchedule, productProgramId.toBuffer()],
    kernelProgramId,
  )[0];
}

function reducedOperatorsAddress(productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.reducedOperators], productProgramId)[0];
}

function policyAddress(kernelProgramId: PublicKey, policyId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.policy, policyId.toBuffer()], kernelProgramId)[0];
}

export function policyReceiptMintAddress(config: ClusterConfig, policyHeader: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [SEEDS.policyReceiptMint, policyHeader.toBuffer()],
    kernelProgramId(config),
  )[0];
}

function policyReceiptAddress(kernelId: PublicKey, policyHeader: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.policyReceipt, policyHeader.toBuffer()], kernelId)[0];
}

function policyReceiptAuthorityAddress(kernelId: PublicKey, policyHeader: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.policyReceiptAuthority, policyHeader.toBuffer()], kernelId)[0];
}

function retailRedemptionRequestAddress(productId: PublicKey, policyHeader: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.retailRedemption, policyHeader.toBuffer()], productId)[0];
}

function termsAddress(productProgramId: PublicKey, policyId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.terms, policyId.toBuffer()], productProgramId)[0];
}

function vaultStateAddress(kernelProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.vaultState], kernelProgramId)[0];
}

function feeLedgerAddress(kernelProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.feeLedger], kernelProgramId)[0];
}

function vaultAuthorityAddress(kernelProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.vaultAuthority], kernelProgramId)[0];
}

function vaultUsdcAddress(kernelProgramId: PublicKey, usdcMint: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.vaultUsdc, usdcMint.toBuffer()], kernelProgramId)[0];
}

function treasuryUsdcAddress(kernelProgramId: PublicKey, usdcMint: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.treasuryUsdc, usdcMint.toBuffer()], kernelProgramId)[0];
}

function altRegistryAddress(kernelProgramId: PublicKey, productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.altRegistry, productProgramId.toBuffer()], kernelProgramId)[0];
}

function couponVaultAddress(kernelProgramId: PublicKey, productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.couponVault, productProgramId.toBuffer()], kernelProgramId)[0];
}

function keeperRegistryAddress(kernelProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.keeperRegistry], kernelProgramId)[0];
}

function hedgeSleeveAddress(kernelProgramId: PublicKey, productProgramId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.hedgeSleeve, productProgramId.toBuffer()], kernelProgramId)[0];
}

function decodeTypedReturnData(
  kind: ProductKind,
  typeName: string,
  returnData: { programId: string; data: [string, string] },
) {
  const bytes = Buffer.from(returnData.data[0], "base64");
  const coder =
    kind === "flagship" ? coders.flagship : kind === "ilProtection" ? coders.il : coders.sol;
  return coder.types.decode(typeName, bytes) as Record<string, unknown>;
}

function decodeReturnData(kind: ProductKind, returnData: { programId: string; data: [string, string] }) {
  return decodeTypedReturnData(kind, "QuotePreview", returnData);
}

function decodeKernelAccount(name: string, data: Buffer) {
  return accountCoders.kernel.decode(name, data) as Record<string, unknown>;
}

function decodeProductTerms(kind: ProductKind, data: Buffer) {
  if (kind === "flagship") return accountCoders.flagship.decode("FlagshipAutocallTerms", data);
  if (kind === "ilProtection") return accountCoders.il.decode("IlProtectionTerms", data);
  return accountCoders.sol.decode("SolAutocallTerms", data);
}

function protocolAdmin(protocolConfig: Record<string, unknown>) {
  const admin = toStringValue(field(protocolConfig, "admin"));
  if (!admin) {
    throw new Error("ProtocolConfig admin is missing");
  }
  return new PublicKey(admin);
}

function configuredUsdcMint(config: ClusterConfig) {
  const value = config.usdcMint?.trim();
  if (!value) return null;
  try {
    return new PublicKey(value);
  } catch {
    throw new Error("Configured USDC mint is not a valid public key");
  }
}

async function usdcMintFromTreasuryDestination(
  connection: Connection,
  protocolConfig: Record<string, unknown>,
) {
  const treasuryDestination = field(protocolConfig, "treasuryDestination");
  if (!treasuryDestination) {
    throw new Error("USDC mint is not configured and ProtocolConfig treasury destination is missing");
  }

  const destination = new PublicKey(toStringValue(treasuryDestination));
  const treasuryAccount = await connection.getAccountInfo(destination, CONFIRMED);
  if (!treasuryAccount?.data?.length || !treasuryAccount.owner.equals(TOKEN_PROGRAM_ID)) {
    throw new Error(
      "USDC mint is not configured and ProtocolConfig treasury destination is not an initialized SPL token account",
    );
  }
  if (treasuryAccount.data.length < 32) {
    throw new Error("ProtocolConfig treasury destination token account data is malformed");
  }
  return new PublicKey(treasuryAccount.data.slice(0, 32));
}

function accountMeta(pubkey: PublicKey, isWritable = false, isSigner = false) {
  return { pubkey, isWritable, isSigner };
}

function previewInstruction(kind: ProductKind, config: ClusterConfig, amount: BN) {
  const kernelId = kernelProgramId(config);
  const productId = programIdForKind(kind, config);
  const protocolConfig = protocolConfigAddress(kernelId);
  const productRegistry = productRegistryAddress(kernelId, productId);
  const vaultSigma = vaultSigmaAddress(kernelId, productId);
  const data =
    kind === "flagship"
      ? instructionCoders.flagship.encode("preview_quote", { notional_usdc: amount })
      : kind === "ilProtection"
        ? instructionCoders.il.encode("preview_quote", { insured_notional_usdc: amount })
        : instructionCoders.sol.encode("preview_quote", { notional_usdc: amount });

  if (kind === "flagship") {
    const feeds = feedAccountsForKind(kind, config) as {
      pythSpy: PublicKey;
      pythQqq: PublicKey;
      pythIwm: PublicKey;
    };
    return new TransactionInstruction({
      programId: productId,
      keys: [
        accountMeta(protocolConfig),
        accountMeta(productRegistry),
        accountMeta(vaultSigma),
        accountMeta(regressionAddress(kernelId)),
        accountMeta(autocallScheduleAddress(kernelId, productId)),
        accountMeta(feeds.pythSpy),
        accountMeta(feeds.pythQqq),
        accountMeta(feeds.pythIwm),
        accountMeta(SYSVAR_CLOCK_PUBKEY),
      ],
      data,
    });
  }

  if (kind === "ilProtection") {
    const feeds = feedAccountsForKind(kind, config) as {
      pythSol: PublicKey;
      pythUsdc: PublicKey;
    };
    return new TransactionInstruction({
      programId: productId,
      keys: [
        accountMeta(protocolConfig),
        accountMeta(productRegistry),
        accountMeta(vaultSigma),
        accountMeta(regimeSignalAddress(kernelId, productId)),
        accountMeta(feeds.pythSol),
        accountMeta(feeds.pythUsdc),
        accountMeta(SYSVAR_CLOCK_PUBKEY),
      ],
      data,
    });
  }

  const feeds = feedAccountsForKind(kind, config) as { pythSol: PublicKey };
  return new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(protocolConfig),
      accountMeta(productRegistry),
      accountMeta(vaultSigma),
      accountMeta(regimeSignalAddress(kernelId, productId)),
      accountMeta(reducedOperatorsAddress(productId)),
      accountMeta(feeds.pythSol),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data,
  });
}

function flagshipLendingValueInstruction(
  config: ClusterConfig,
  policyAddress: PublicKey,
  productTermsAddress: PublicKey,
) {
  const kernelId = kernelProgramId(config);
  const productId = programIdForKind("flagship", config);
  const feeds = feedAccountsForKind("flagship", config) as {
    pythSpy: PublicKey;
    pythQqq: PublicKey;
    pythIwm: PublicKey;
  };

  return new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(protocolConfigAddress(kernelId)),
      accountMeta(vaultSigmaAddress(kernelId, productId)),
      accountMeta(regressionAddress(kernelId)),
      accountMeta(policyAddress),
      accountMeta(productTermsAddress),
      accountMeta(feeds.pythSpy),
      accountMeta(feeds.pythQqq),
      accountMeta(feeds.pythIwm),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data: instructionCoders.flagship.encode("preview_lending_value", {}),
  });
}

async function simulateFlagshipLendingValue(
  connection: Connection,
  config: ClusterConfig,
  payer: PublicKey,
  policyAddress: PublicKey,
  productTermsAddress: PublicKey,
) {
  const ix = flagshipLendingValueInstruction(config, policyAddress, productTermsAddress);
  const blockhash = await connection.getLatestBlockhash(CONFIRMED);
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: blockhash.blockhash,
    instructions: withComputeBudget([ix]),
  }).compileToV0Message([]);
  const tx = new VersionedTransaction(message);
  const result = await connection.simulateTransaction(tx, {
    sigVerify: false,
    replaceRecentBlockhash: true,
    commitment: CONFIRMED,
  });
  if (result.value.err || !result.value.returnData) return null;
  return decodeTypedReturnData("flagship", "LendingValuePreview", result.value.returnData);
}

function checkpointChunkCandidates(maxChunkSize = MIDLIFE_CHECKPOINT_MAX_CHUNK_SIZE) {
  return [...new Set([maxChunkSize, 12, 9, 6, 4, 3, 2, 1])]
    .filter((chunkSize) => chunkSize >= 1 && chunkSize <= maxChunkSize)
    .sort((lhs, rhs) => rhs - lhs);
}

function nextMidlifeCheckpointStop(currentCouponIndex: number, chunkSize: number) {
  return Math.min(MIDLIFE_FINAL_COUPON_INDEX, currentCouponIndex + chunkSize);
}

async function buildVersionedTransaction(
  connection: Connection,
  payer: PublicKey,
  instructions: TransactionInstruction[],
) {
  const latestBlockhash = await connection.getLatestBlockhash(CONFIRMED);
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: latestBlockhash.blockhash,
    instructions: withComputeBudget(instructions),
  }).compileToV0Message([]);
  return new VersionedTransaction(message);
}

async function simulateVersionedTransaction(
  connection: Connection,
  transaction: VersionedTransaction,
  signers: Keypair[] = [],
) {
  if (signers.length > 0) transaction.sign(signers);
  const result = await connection.simulateTransaction(transaction, {
    sigVerify: false,
    replaceRecentBlockhash: true,
    commitment: CONFIRMED,
  });
  if (result.value.err) {
    const logs = result.value.logs?.join("\n") ?? "";
    throw new Error(`Simulation failed: ${JSON.stringify(result.value.err)}\n${logs}`);
  }
  return result.value;
}

function simulatedUnitsConsumed(result: { unitsConsumed?: number | null }) {
  return result.unitsConsumed ?? FRONTEND_COMPUTE_UNIT_LIMIT;
}

function decodeCheckpointPreview(returnData: { programId: string; data: [string, string] } | null | undefined) {
  if (!returnData) throw new Error("Checkpoint simulation returned no Anchor return data");
  return decodeTypedReturnData("flagship", "MidlifeNavCheckpointPreview", returnData);
}

function decodeLendingValuePreview(returnData: { programId: string; data: [string, string] } | null | undefined) {
  if (!returnData) throw new Error("Checkpoint finish returned no Anchor lending-value preview");
  return decodeTypedReturnData("flagship", "LendingValuePreview", returnData);
}

async function simulateCheckpointPreviewTransaction(
  connection: Connection,
  transaction: VersionedTransaction,
  signers: Keypair[] = [],
) {
  const result = await simulateVersionedTransaction(connection, transaction, signers);
  return {
    preview: decodeCheckpointPreview(result.returnData),
    unitsConsumed: simulatedUnitsConsumed(result),
  };
}

function flagshipPrepareMidlifeNavInstruction(
  config: ClusterConfig,
  requester: PublicKey,
  checkpoint: PublicKey,
  policyAddress: PublicKey,
  productTermsAddress: PublicKey,
  stopCouponIndex: number,
) {
  const kernelId = kernelProgramId(config);
  const productId = programIdForKind("flagship", config);
  const feeds = feedAccountsForKind("flagship", config) as {
    pythSpy: PublicKey;
    pythQqq: PublicKey;
    pythIwm: PublicKey;
  };

  return new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(requester, false, true),
      accountMeta(checkpoint, true),
      accountMeta(protocolConfigAddress(kernelId)),
      accountMeta(vaultSigmaAddress(kernelId, productId)),
      accountMeta(regressionAddress(kernelId)),
      accountMeta(policyAddress),
      accountMeta(productTermsAddress),
      accountMeta(feeds.pythSpy),
      accountMeta(feeds.pythQqq),
      accountMeta(feeds.pythIwm),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data: instructionCoders.flagship.encode("prepare_midlife_nav", {
      stop_coupon_index: stopCouponIndex,
    }),
  });
}

function flagshipAdvanceMidlifeNavInstruction(
  config: ClusterConfig,
  requester: PublicKey,
  checkpoint: PublicKey,
  policyAddress: PublicKey,
  productTermsAddress: PublicKey,
  stopCouponIndex: number,
) {
  const productId = programIdForKind("flagship", config);
  return new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(requester, false, true),
      accountMeta(checkpoint, true),
      accountMeta(policyAddress),
      accountMeta(productTermsAddress),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data: instructionCoders.flagship.encode("advance_midlife_nav", {
      stop_coupon_index: stopCouponIndex,
    }),
  });
}

function flagshipPreviewLendingValueFromCheckpointInstruction(
  config: ClusterConfig,
  requester: PublicKey,
  checkpoint: PublicKey,
  policyAddress: PublicKey,
  productTermsAddress: PublicKey,
) {
  const productId = programIdForKind("flagship", config);
  return new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(requester, true, true),
      accountMeta(checkpoint, true),
      accountMeta(policyAddress),
      accountMeta(productTermsAddress),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data: instructionCoders.flagship.encode("preview_lending_value_from_checkpoint", {}),
  });
}

function flagshipBuybackFromCheckpointInstruction(
  config: ClusterConfig,
  policyOwner: PublicKey,
  checkpoint: PublicKey,
  policyAddress: PublicKey,
  productTermsAddress: PublicKey,
  protocolContext: ProtocolContext,
) {
  const kernelId = protocolContext.kernelProgramId;
  const productId = protocolContext.productProgramId;
  return new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(policyOwner, true, true),
      accountMeta(checkpoint, true),
      accountMeta(policyAddress, true),
      accountMeta(productTermsAddress, true),
      accountMeta(protocolContext.productRegistryAddress, true),
      accountMeta(protocolContext.protocolConfigAddress),
      accountMeta(protocolContext.usdcMint),
      accountMeta(vaultUsdcAddress(kernelId, protocolContext.usdcMint), true),
      accountMeta(vaultAuthorityAddress(kernelId)),
      accountMeta(getAssociatedTokenAddressSync(protocolContext.usdcMint, policyOwner), true),
      accountMeta(productAuthority(productId)),
      accountMeta(vaultStateAddress(kernelId), true),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
      accountMeta(kernelId),
      accountMeta(TOKEN_PROGRAM_ID),
    ],
    data: instructionCoders.flagship.encode("buyback_from_checkpoint", {}),
  });
}

function unwrapPolicyReceiptInstruction(
  config: ClusterConfig,
  holder: PublicKey,
  policyHeader: PublicKey,
) {
  const kernelId = kernelProgramId(config);
  const receiptMint = policyReceiptMintAddress(config, policyHeader);
  return new TransactionInstruction({
    programId: kernelId,
    keys: [
      accountMeta(holder, true, true),
      accountMeta(policyHeader, true),
      accountMeta(policyReceiptAddress(kernelId, policyHeader), true),
      accountMeta(receiptMint, true),
      accountMeta(policyReceiptAuthorityAddress(kernelId, policyHeader)),
      accountMeta(getAssociatedTokenAddressSync(receiptMint, holder), true),
      accountMeta(TOKEN_PROGRAM_ID),
    ],
    data: instructionCoders.kernel.encode("unwrap_policy_receipt", {}),
  });
}

function mockLendingMarkerInstructions(
  payer: PublicKey,
  markerRecipient: PublicKey,
  memo: string,
  includeMemo: boolean,
) {
  const instructions = [
    SystemProgram.transfer({
      fromPubkey: payer,
      toPubkey: markerRecipient,
      lamports: 1,
    }),
  ];

  if (includeMemo) {
    instructions.push(
      new TransactionInstruction({
        programId: MEMO_PROGRAM_ID,
        keys: [],
        data: Buffer.from(memo.slice(0, 512), "utf8"),
      }),
    );
  }
  return instructions;
}

async function fetchFlagshipTerms(
  connection: Connection,
  productTermsAddress: PublicKey,
) {
  const info = await connection.getAccountInfo(productTermsAddress, CONFIRMED);
  if (!info?.data) throw new Error("Flagship product terms account not found");
  return decodeProductTerms("flagship", info.data as Buffer) as Record<string, unknown>;
}

async function buildPrepareMidlifeCheckpointTransaction(
  connection: Connection,
  config: ClusterConfig,
  payer: PublicKey,
  checkpoint: PublicKey,
  policyAddress: PublicKey,
  productTermsAddress: PublicKey,
  stopCouponIndex: number,
) {
  const lamports = await connection.getMinimumBalanceForRentExemption(
    MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE,
    CONFIRMED,
  );
  return buildVersionedTransaction(connection, payer, [
    SystemProgram.createAccount({
      fromPubkey: payer,
      newAccountPubkey: checkpoint,
      lamports,
      space: MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE,
      programId: programIdForKind("flagship", config),
    }),
    flagshipPrepareMidlifeNavInstruction(
      config,
      payer,
      checkpoint,
      policyAddress,
      productTermsAddress,
      stopCouponIndex,
    ),
  ]);
}

async function executeCheckpointedFlagshipPath({
  connection,
  config,
  payer,
  policyAddress,
  productTermsAddress,
  sendTransaction,
  buildFinalInstructions,
  decodePreview,
}: {
  connection: Connection;
  config: ClusterConfig;
  payer: PublicKey;
  policyAddress: PublicKey;
  productTermsAddress: PublicKey;
  sendTransaction: CheckpointTransactionSender;
  buildFinalInstructions: (checkpoint: PublicKey) => Promise<TransactionInstruction[]> | TransactionInstruction[];
  decodePreview: boolean;
}) {
  const terms = await fetchFlagshipTerms(connection, productTermsAddress);
  const checkpoint = Keypair.generate();
  const signatures: string[] = [];
  const transactionUnitsConsumed: number[] = [];
  const advanceChunkSizes: number[] = [];
  const chunkCandidates = checkpointChunkCandidates();
  let currentCouponIndex = Math.min(
    MIDLIFE_FINAL_COUPON_INDEX,
    Math.max(0, toNumber(field(terms, "nextCouponIndex"))),
  );

  let selectedPrepare:
    | {
        transaction: VersionedTransaction;
        chunkSize: number;
        nextCouponIndex: number;
        unitsConsumed: number;
      }
    | null = null;
  let lastPrepareError: unknown = null;

  for (const chunkSize of chunkCandidates) {
    const stopCouponIndex = nextMidlifeCheckpointStop(currentCouponIndex, chunkSize);
    const transaction = await buildPrepareMidlifeCheckpointTransaction(
      connection,
      config,
      payer,
      checkpoint.publicKey,
      policyAddress,
      productTermsAddress,
      stopCouponIndex,
    );
    try {
      const simulation = await simulateCheckpointPreviewTransaction(connection, transaction, [checkpoint]);
      if (simulation.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
        lastPrepareError = new Error(
          `prepare checkpoint chunk ${chunkSize} exceeded ${MIDLIFE_CHECKPOINT_TARGET_UNITS} CU: ${simulation.unitsConsumed}`,
        );
        continue;
      }
      selectedPrepare = {
        transaction,
        chunkSize,
        nextCouponIndex: toNumber(field(simulation.preview, "nextCouponIndex")),
        unitsConsumed: simulation.unitsConsumed,
      };
      break;
    } catch (cause) {
      lastPrepareError = cause;
    }
  }

  if (!selectedPrepare) {
    throw new Error(
      `Unable to prepare midlife NAV checkpoint: ${
        lastPrepareError instanceof Error ? lastPrepareError.message : String(lastPrepareError)
      }`,
    );
  }

  signatures.push(await sendTransaction(selectedPrepare.transaction, [checkpoint]));
  transactionUnitsConsumed.push(selectedPrepare.unitsConsumed);
  currentCouponIndex = selectedPrepare.nextCouponIndex;

  let pendingAdvanceInstructions: TransactionInstruction[] = [];

  async function sendPendingAdvanceBatch() {
    if (pendingAdvanceInstructions.length === 0) return;
    const transaction = await buildVersionedTransaction(connection, payer, pendingAdvanceInstructions);
    const simulation = await simulateVersionedTransaction(connection, transaction);
    const unitsConsumed = simulatedUnitsConsumed(simulation);
    if (unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
      throw new Error(`checkpoint advance batch exceeded ${MIDLIFE_CHECKPOINT_TARGET_UNITS} CU: ${unitsConsumed}`);
    }
    signatures.push(await sendTransaction(transaction, []));
    transactionUnitsConsumed.push(unitsConsumed);
    pendingAdvanceInstructions = [];
  }

  let guard = 0;
  while (currentCouponIndex < MIDLIFE_FINAL_COUPON_INDEX) {
    guard += 1;
    if (guard > 64) throw new Error("Midlife checkpoint planner did not make bounded progress");

    let selected:
      | {
          instruction: TransactionInstruction;
          chunkSize: number;
          nextCouponIndex: number;
        }
      | null = null;

    for (const chunkSize of chunkCandidates) {
      const stopCouponIndex = nextMidlifeCheckpointStop(currentCouponIndex, chunkSize);
      const instruction = flagshipAdvanceMidlifeNavInstruction(
        config,
        payer,
        checkpoint.publicKey,
        policyAddress,
        productTermsAddress,
        stopCouponIndex,
      );
      const transaction = await buildVersionedTransaction(connection, payer, [
        ...pendingAdvanceInstructions,
        instruction,
      ]);
      try {
        const simulation = await simulateCheckpointPreviewTransaction(connection, transaction);
        if (simulation.unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) continue;
        selected = {
          instruction,
          chunkSize,
          nextCouponIndex: toNumber(field(simulation.preview, "nextCouponIndex")),
        };
        break;
      } catch {
        // Try a smaller deterministic chunk.
      }
    }

    if (!selected) {
      if (pendingAdvanceInstructions.length === 0) {
        throw new Error(`Unable to advance checkpoint from coupon ${currentCouponIndex}`);
      }
      await sendPendingAdvanceBatch();
      continue;
    }

    pendingAdvanceInstructions.push(selected.instruction);
    advanceChunkSizes.push(selected.chunkSize);
    currentCouponIndex = selected.nextCouponIndex;
  }

  const finalInstructions = await buildFinalInstructions(checkpoint.publicKey);
  let finalTransaction = await buildVersionedTransaction(connection, payer, [
    ...pendingAdvanceInstructions,
    ...finalInstructions,
  ]);
  let finalSimulation:
    | {
        preview: Record<string, unknown> | null;
        unitsConsumed: number;
      }
    | null = null;

  try {
    const simulation = await simulateVersionedTransaction(connection, finalTransaction);
    const unitsConsumed = simulatedUnitsConsumed(simulation);
    if (unitsConsumed <= MIDLIFE_CHECKPOINT_TARGET_UNITS) {
      finalSimulation = {
        preview: decodePreview ? decodeLendingValuePreview(simulation.returnData) : null,
        unitsConsumed,
      };
    }
  } catch {
    finalSimulation = null;
  }

  if (!finalSimulation && pendingAdvanceInstructions.length > 0) {
    await sendPendingAdvanceBatch();
    finalTransaction = await buildVersionedTransaction(connection, payer, finalInstructions);
    const simulation = await simulateVersionedTransaction(connection, finalTransaction);
    const unitsConsumed = simulatedUnitsConsumed(simulation);
    if (unitsConsumed > MIDLIFE_CHECKPOINT_TARGET_UNITS) {
      throw new Error(`checkpoint finish exceeded ${MIDLIFE_CHECKPOINT_TARGET_UNITS} CU: ${unitsConsumed}`);
    }
    finalSimulation = {
      preview: decodePreview ? decodeLendingValuePreview(simulation.returnData) : null,
      unitsConsumed,
    };
  }

  if (!finalSimulation) {
    throw new Error("Unable to finish midlife checkpoint inside the configured CU target");
  }

  signatures.push(await sendTransaction(finalTransaction, []));
  transactionUnitsConsumed.push(finalSimulation.unitsConsumed);

  return {
    preview: finalSimulation.preview,
    checkpoint: checkpoint.publicKey,
    signatures,
    prepareChunkSize: selectedPrepare.chunkSize,
    advanceChunkSizes,
    transactionUnitsConsumed,
    maxUnitsConsumed: Math.max(...transactionUnitsConsumed),
  };
}

export async function executeCheckpointedFlagshipLendingValue({
  connection,
  config,
  payer,
  policyAddress,
  productTermsAddress,
  sendTransaction,
  finalExtraInstructions = [],
}: {
  connection: Connection;
  config: ClusterConfig;
  payer: PublicKey;
  policyAddress: PublicKey;
  productTermsAddress: PublicKey;
  sendTransaction: CheckpointTransactionSender;
  finalExtraInstructions?: TransactionInstruction[];
}): Promise<CheckpointedLendingValueExecution> {
  const result = await executeCheckpointedFlagshipPath({
    connection,
    config,
    payer,
    policyAddress,
    productTermsAddress,
    sendTransaction,
    decodePreview: true,
    buildFinalInstructions: (checkpoint) => [
      flagshipPreviewLendingValueFromCheckpointInstruction(
        config,
        payer,
        checkpoint,
        policyAddress,
        productTermsAddress,
      ),
      ...finalExtraInstructions,
    ],
  });

  if (!result.preview) {
    throw new Error("Checkpointed lending value completed without a decoded preview");
  }

  return result as CheckpointedLendingValueExecution;
}

export async function executeCheckpointedMockLendingBorrow({
  connection,
  config,
  payer,
  markerRecipient,
  memo,
  includeMemo,
  policyAddress,
  productTermsAddress,
  sendTransaction,
}: {
  connection: Connection;
  config: ClusterConfig;
  payer: PublicKey;
  markerRecipient: PublicKey;
  memo: string;
  includeMemo: boolean;
  policyAddress: PublicKey;
  productTermsAddress: PublicKey;
  sendTransaction: CheckpointTransactionSender;
}) {
  return executeCheckpointedFlagshipLendingValue({
    connection,
    config,
    payer,
    policyAddress,
    productTermsAddress,
    sendTransaction,
    finalExtraInstructions: mockLendingMarkerInstructions(payer, markerRecipient, memo, includeMemo),
  });
}

export async function executeCheckpointedWrappedFlagshipLiquidation({
  connection,
  config,
  holder,
  policyAddress,
  productTermsAddress,
  sendTransaction,
}: {
  connection: Connection;
  config: ClusterConfig;
  holder: PublicKey;
  policyAddress: PublicKey;
  productTermsAddress: PublicKey;
  sendTransaction: CheckpointTransactionSender;
}) {
  const protocolContext = await fetchProtocolContext(connection, config, "flagship");
  return executeCheckpointedFlagshipPath({
    connection,
    config,
    payer: holder,
    policyAddress,
    productTermsAddress,
    sendTransaction,
    decodePreview: false,
    buildFinalInstructions: (checkpoint) => [
      unwrapPolicyReceiptInstruction(config, holder, policyAddress),
      flagshipBuybackFromCheckpointInstruction(
        config,
        holder,
        checkpoint,
        policyAddress,
        productTermsAddress,
        protocolContext,
      ),
    ],
  });
}

export async function fetchProtocolContext(
  connection: Connection,
  config: ClusterConfig,
  kind: ProductKind,
) {
  const kernelId = kernelProgramId(config);
  const productId = programIdForKind(kind, config);
  const protocolAddress = protocolConfigAddress(kernelId);
  const registryAddress = productRegistryAddress(kernelId, productId);
  const [protocolInfo, registryInfo] = await connection.getMultipleAccountsInfo(
    [protocolAddress, registryAddress],
    CONFIRMED,
  );

  if (!protocolInfo?.data) {
    throw new Error("ProtocolConfig account not found");
  }
  if (!registryInfo?.data) {
    throw new Error("ProductRegistryEntry account not found");
  }

  const protocolConfig = decodeKernelAccount("ProtocolConfig", protocolInfo.data);
  const productRegistry = decodeKernelAccount("ProductRegistryEntry", registryInfo.data);
  const usdcMint = configuredUsdcMint(config) ?? (await usdcMintFromTreasuryDestination(connection, protocolConfig));

  return {
    kernelProgramId: kernelId,
    productProgramId: productId,
    protocolConfigAddress: protocolAddress,
    protocolConfig,
    productRegistryAddress: registryAddress,
    productRegistry,
    usdcMint,
  } satisfies ProtocolContext;
}

export async function simulatePreview(
  connection: Connection,
  config: ClusterConfig,
  kind: ProductKind,
  amountBaseUnits: BN,
) {
  const protocolContext = await fetchProtocolContext(connection, config, kind);
  const ix = previewInstruction(kind, config, amountBaseUnits);
  const blockhash = await connection.getLatestBlockhash(CONFIRMED);
  const payer = protocolAdmin(protocolContext.protocolConfig);
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: blockhash.blockhash,
    instructions: withComputeBudget([ix]),
  }).compileToV0Message([]);
  const tx = new VersionedTransaction(message);
  const result = await connection.simulateTransaction(tx, {
    sigVerify: false,
    replaceRecentBlockhash: true,
    commitment: CONFIRMED,
  });
  if (result.value.err) {
    const error = new Error(`Preview failed: ${JSON.stringify(result.value.err)}`) as Error & {
      logs?: string[];
    };
    error.logs = result.value.logs ?? [];
    throw error;
  }
  if (!result.value.returnData) {
    throw new Error("Preview returned no Anchor return data");
  }
  return {
    data: decodeReturnData(kind, result.value.returnData),
    protocolContext,
  } satisfies ProductPreviewResult;
}

type BuyBounds = {
  slippageBps: number;
  maxQuoteSlotDelta: number;
  maxEntryPriceDeviationBps: number;
  maxExpiryDeltaSecs: number;
};

export async function buildBuyTransaction(
  connection: Connection,
  config: ClusterConfig,
  kind: ProductKind,
  buyer: PublicKey,
  preview: ProductPreviewResult,
  notionalBaseUnits: BN,
  bounds: BuyBounds,
) {
  const protocolContext = preview.protocolContext;
  const kernelId = protocolContext.kernelProgramId;
  const productId = protocolContext.productProgramId;
  const policyId = Keypair.generate().publicKey;
  const policy = policyAddress(kernelId, policyId);
  const productTerms = termsAddress(productId, policyId);
  const buyerUsdc = getAssociatedTokenAddressSync(protocolContext.usdcMint, buyer);
  const vaultUsdc = vaultUsdcAddress(kernelId, protocolContext.usdcMint);
  const treasuryUsdc = treasuryUsdcAddress(kernelId, protocolContext.usdcMint);
  const vaultAuthority = vaultAuthorityAddress(kernelId);
  const vaultSigma = vaultSigmaAddress(kernelId, productId);
  const vaultState = vaultStateAddress(kernelId);
  const feeLedger = feeLedgerAddress(kernelId);
  const productRegistry = productRegistryAddress(kernelId, productId);
  const previewData = preview.data;
  const premium = new BN(toNumber(field(previewData, "premium")));
  const maxLiability = new BN(toNumber(field(previewData, "maxLiability")));
  const maxPremium = premium.mul(new BN(10_000 + bounds.slippageBps)).div(new BN(10_000));
  const minMaxLiability = maxLiability.mul(new BN(10_000 - bounds.slippageBps)).div(new BN(10_000));

  let data: Buffer;
  let keys: Array<{ pubkey: PublicKey; isWritable: boolean; isSigner: boolean }>;

  if (kind === "flagship") {
    const feeds = feedAccountsForKind(kind, config) as {
      pythSpy: PublicKey;
      pythQqq: PublicKey;
      pythIwm: PublicKey;
    };
    data = instructionCoders.flagship.encode("accept_quote", {
      args: {
        policy_id: policyId,
        notional_usdc: notionalBaseUnits,
        max_premium: maxPremium,
        min_max_liability: minMaxLiability,
        min_offered_coupon_bps_s6: new BN(toNumber(field(previewData, "offeredCouponBpsS6"))),
        preview_quote_slot: new BN(toNumber(field(previewData, "quoteSlot"))),
        max_quote_slot_delta: new BN(bounds.maxQuoteSlotDelta),
        preview_entry_spy_price_s6: new BN(toNumber(field(previewData, "entrySpyPriceS6"))),
        preview_entry_qqq_price_s6: new BN(toNumber(field(previewData, "entryQqqPriceS6"))),
        preview_entry_iwm_price_s6: new BN(toNumber(field(previewData, "entryIwmPriceS6"))),
        max_entry_price_deviation_bps: bounds.maxEntryPriceDeviationBps,
        preview_expiry_ts: new BN(toNumber(field(previewData, "expiryTs"))),
        max_expiry_delta_secs: new BN(bounds.maxExpiryDeltaSecs),
      },
    });
    keys = [
      accountMeta(buyer, true, true),
      accountMeta(policy, true),
      accountMeta(productTerms, true),
      accountMeta(productAuthority(productId)),
      accountMeta(protocolContext.usdcMint),
      accountMeta(buyerUsdc, true),
      accountMeta(vaultUsdc, true),
      accountMeta(treasuryUsdc, true),
      accountMeta(vaultAuthority),
      accountMeta(protocolContext.protocolConfigAddress, true),
      accountMeta(vaultSigma),
      accountMeta(regressionAddress(kernelId)),
      accountMeta(autocallScheduleAddress(kernelId, productId)),
      accountMeta(feeds.pythSpy),
      accountMeta(feeds.pythQqq),
      accountMeta(feeds.pythIwm),
      accountMeta(vaultState, true),
      accountMeta(feeLedger, true),
      accountMeta(productRegistry, true),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
      accountMeta(kernelId),
      accountMeta(TOKEN_PROGRAM_ID),
      accountMeta(SystemProgram.programId),
    ];
  } else if (kind === "ilProtection") {
    const feeds = feedAccountsForKind(kind, config) as {
      pythSol: PublicKey;
      pythUsdc: PublicKey;
    };
    data = instructionCoders.il.encode("accept_quote", {
      args: {
        policy_id: policyId,
        insured_notional_usdc: notionalBaseUnits,
        max_premium: maxPremium,
        min_max_liability: minMaxLiability,
        preview_quote_slot: new BN(toNumber(field(previewData, "quoteSlot"))),
        max_quote_slot_delta: new BN(bounds.maxQuoteSlotDelta),
        preview_entry_sol_price_s6: new BN(toNumber(field(previewData, "entrySolPriceS6"))),
        preview_entry_usdc_price_s6: new BN(toNumber(field(previewData, "entryUsdcPriceS6"))),
        max_entry_price_deviation_bps: bounds.maxEntryPriceDeviationBps,
        preview_expiry_ts: new BN(toNumber(field(previewData, "expiryTs"))),
        max_expiry_delta_secs: new BN(bounds.maxExpiryDeltaSecs),
      },
    });
    keys = [
      accountMeta(buyer, true, true),
      accountMeta(policy, true),
      accountMeta(productTerms, true),
      accountMeta(productAuthority(productId)),
      accountMeta(protocolContext.usdcMint),
      accountMeta(buyerUsdc, true),
      accountMeta(vaultUsdc, true),
      accountMeta(treasuryUsdc, true),
      accountMeta(vaultAuthority),
      accountMeta(protocolContext.protocolConfigAddress, true),
      accountMeta(vaultSigma),
      accountMeta(regimeSignalAddress(kernelId, productId)),
      accountMeta(feeds.pythSol),
      accountMeta(feeds.pythUsdc),
      accountMeta(vaultState, true),
      accountMeta(feeLedger, true),
      accountMeta(productRegistry, true),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
      accountMeta(kernelId),
      accountMeta(TOKEN_PROGRAM_ID),
      accountMeta(SystemProgram.programId),
    ];
  } else {
    const feeds = feedAccountsForKind(kind, config) as { pythSol: PublicKey };
    data = instructionCoders.sol.encode("accept_quote", {
      args: {
        policy_id: policyId,
        notional_usdc: notionalBaseUnits,
        max_premium: maxPremium,
        min_max_liability: minMaxLiability,
        min_offered_coupon_bps_s6: new BN(toNumber(field(previewData, "offeredCouponBpsS6"))),
        preview_quote_slot: new BN(toNumber(field(previewData, "quoteSlot"))),
        max_quote_slot_delta: new BN(bounds.maxQuoteSlotDelta),
        preview_entry_price_s6: new BN(toNumber(field(previewData, "entryPriceS6"))),
        max_entry_price_deviation_bps: bounds.maxEntryPriceDeviationBps,
        preview_expiry_ts: new BN(toNumber(field(previewData, "expiryTs"))),
        max_expiry_delta_secs: new BN(bounds.maxExpiryDeltaSecs),
      },
    });
    keys = [
      accountMeta(buyer, true, true),
      accountMeta(policy, true),
      accountMeta(productTerms, true),
      accountMeta(productAuthority(productId)),
      accountMeta(protocolContext.usdcMint),
      accountMeta(buyerUsdc, true),
      accountMeta(vaultUsdc, true),
      accountMeta(treasuryUsdc, true),
      accountMeta(vaultAuthority),
      accountMeta(protocolContext.protocolConfigAddress, true),
      accountMeta(vaultSigma),
      accountMeta(regimeSignalAddress(kernelId, productId)),
      accountMeta(reducedOperatorsAddress(productId)),
      accountMeta(feeds.pythSol),
      accountMeta(vaultState, true),
      accountMeta(feeLedger, true),
      accountMeta(productRegistry, true),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
      accountMeta(kernelId),
      accountMeta(TOKEN_PROGRAM_ID),
      accountMeta(SystemProgram.programId),
    ];
  }

  const instruction = new TransactionInstruction({
    programId: productId,
    keys,
    data,
  });

  const lookupTables = await loadLookupTables(connection, kernelId, productId);
  const latestBlockhash = await connection.getLatestBlockhash(CONFIRMED);
  const message = new TransactionMessage({
    payerKey: buyer,
    recentBlockhash: latestBlockhash.blockhash,
    instructions: withComputeBudget([instruction]),
  }).compileToV0Message(lookupTables);

  return {
    policyId,
    transaction: new VersionedTransaction(message),
  };
}

export async function buildWrapPolicyReceiptTransaction(
  connection: Connection,
  config: ClusterConfig,
  holder: PublicKey,
  policyHeader: PublicKey,
) {
  const kernelId = kernelProgramId(config);
  const receiptMint = policyReceiptMintAddress(config, policyHeader);
  const instruction = new TransactionInstruction({
    programId: kernelId,
    keys: [
      accountMeta(holder, true, true),
      accountMeta(policyHeader, true),
      accountMeta(policyReceiptAddress(kernelId, policyHeader), true),
      accountMeta(receiptMint, true),
      accountMeta(policyReceiptAuthorityAddress(kernelId, policyHeader)),
      accountMeta(getAssociatedTokenAddressSync(receiptMint, holder), true),
      accountMeta(TOKEN_PROGRAM_ID),
      accountMeta(ASSOCIATED_TOKEN_PROGRAM_ID),
      accountMeta(SystemProgram.programId),
    ],
    data: instructionCoders.kernel.encode("wrap_policy_receipt", {}),
  });
  return buildSingleInstructionTransaction(connection, holder, [instruction]);
}

export async function buildRequestRetailRedemptionTransaction(
  connection: Connection,
  config: ClusterConfig,
  owner: PublicKey,
  policyHeader: PublicKey,
  productTerms: PublicKey,
) {
  const productId = programIdForKind("flagship", config);
  const instruction = new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(owner, true, true),
      accountMeta(policyHeader),
      accountMeta(productTerms),
      accountMeta(retailRedemptionRequestAddress(productId, policyHeader), true),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
      accountMeta(SystemProgram.programId),
    ],
    data: instructionCoders.flagship.encode("request_retail_redemption", {}),
  });
  return buildSingleInstructionTransaction(connection, owner, [instruction]);
}

export async function buildExecuteRetailRedemptionTransaction(
  connection: Connection,
  config: ClusterConfig,
  owner: PublicKey,
  policyHeader: PublicKey,
  productTerms: PublicKey,
) {
  const protocolContext = await fetchProtocolContext(connection, config, "flagship");
  const kernelId = protocolContext.kernelProgramId;
  const productId = protocolContext.productProgramId;
  const feeds = feedAccountsForKind("flagship", config) as {
    pythSpy: PublicKey;
    pythQqq: PublicKey;
    pythIwm: PublicKey;
  };

  const instruction = new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(owner, false, true),
      accountMeta(policyHeader, true),
      accountMeta(productTerms, true),
      accountMeta(retailRedemptionRequestAddress(productId, policyHeader), true),
      accountMeta(protocolContext.productRegistryAddress, true),
      accountMeta(protocolContext.protocolConfigAddress),
      accountMeta(vaultSigmaAddress(kernelId, productId)),
      accountMeta(regressionAddress(kernelId)),
      accountMeta(feeds.pythSpy),
      accountMeta(feeds.pythQqq),
      accountMeta(feeds.pythIwm),
      accountMeta(protocolContext.usdcMint),
      accountMeta(vaultUsdcAddress(kernelId, protocolContext.usdcMint), true),
      accountMeta(vaultAuthorityAddress(kernelId)),
      accountMeta(getAssociatedTokenAddressSync(protocolContext.usdcMint, owner), true),
      accountMeta(productAuthority(productId)),
      accountMeta(vaultStateAddress(kernelId), true),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
      accountMeta(kernelId),
      accountMeta(TOKEN_PROGRAM_ID),
    ],
    data: instructionCoders.flagship.encode("execute_retail_redemption", {}),
  });
  return buildSingleInstructionTransaction(connection, owner, [instruction]);
}

export async function buildLiquidateWrappedFlagshipTransaction(
  connection: Connection,
  config: ClusterConfig,
  holder: PublicKey,
  policyHeader: PublicKey,
  productTerms: PublicKey,
) {
  const protocolContext = await fetchProtocolContext(connection, config, "flagship");
  const kernelId = protocolContext.kernelProgramId;
  const productId = protocolContext.productProgramId;
  const receiptMint = policyReceiptMintAddress(config, policyHeader);
  const feeds = feedAccountsForKind("flagship", config) as {
    pythSpy: PublicKey;
    pythQqq: PublicKey;
    pythIwm: PublicKey;
  };

  const unwrapInstruction = new TransactionInstruction({
    programId: kernelId,
    keys: [
      accountMeta(holder, true, true),
      accountMeta(policyHeader, true),
      accountMeta(policyReceiptAddress(kernelId, policyHeader), true),
      accountMeta(receiptMint, true),
      accountMeta(policyReceiptAuthorityAddress(kernelId, policyHeader)),
      accountMeta(getAssociatedTokenAddressSync(receiptMint, holder), true),
      accountMeta(TOKEN_PROGRAM_ID),
    ],
    data: instructionCoders.kernel.encode("unwrap_policy_receipt", {}),
  });

  const buybackInstruction = new TransactionInstruction({
    programId: productId,
    keys: [
      accountMeta(holder, false, true),
      accountMeta(policyHeader, true),
      accountMeta(productTerms, true),
      accountMeta(protocolContext.productRegistryAddress, true),
      accountMeta(protocolContext.protocolConfigAddress),
      accountMeta(vaultSigmaAddress(kernelId, productId)),
      accountMeta(regressionAddress(kernelId)),
      accountMeta(feeds.pythSpy),
      accountMeta(feeds.pythQqq),
      accountMeta(feeds.pythIwm),
      accountMeta(protocolContext.usdcMint),
      accountMeta(vaultUsdcAddress(kernelId, protocolContext.usdcMint), true),
      accountMeta(vaultAuthorityAddress(kernelId)),
      accountMeta(getAssociatedTokenAddressSync(protocolContext.usdcMint, holder), true),
      accountMeta(productAuthority(productId)),
      accountMeta(vaultStateAddress(kernelId), true),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
      accountMeta(kernelId),
      accountMeta(TOKEN_PROGRAM_ID),
    ],
    data: instructionCoders.flagship.encode("buyback", {}),
  });

  return buildSingleInstructionTransaction(connection, holder, [unwrapInstruction, buybackInstruction]);
}

export async function buildMockLendingMarkerTransaction(
  connection: Connection,
  payer: PublicKey,
  markerRecipient: PublicKey,
  memo: string,
  includeMemo: boolean,
) {
  const instructions = [
    SystemProgram.transfer({
      fromPubkey: payer,
      toPubkey: markerRecipient,
      lamports: 1,
    }),
  ];

  if (includeMemo) {
    instructions.push(
      new TransactionInstruction({
        programId: MEMO_PROGRAM_ID,
        keys: [],
        data: Buffer.from(memo.slice(0, 512), "utf8"),
      }),
    );
  }

  return buildSingleInstructionTransaction(connection, payer, instructions);
}

export type MockLendingBorrowPricing =
  | {
      mode: "flagshipQuote";
      notionalBaseUnits: BN;
    }
  | {
      mode: "flagshipLendingValue";
      policyAddress: PublicKey;
      productTermsAddress: PublicKey;
    };

export interface DemoPriceAndIssueLoanParams {
  receiptMint: PublicKey;
  borrower?: PublicKey;
  loanId: BN;
  notionalBaseUnits: BN;
  fairValueBaseUnits: BN;
  lendingValueBaseUnits: BN;
  maxBorrowBaseUnits: BN;
  debtBaseUnits: BN;
  sourceSlot: BN;
  includeMemo?: boolean;
}

export async function buildDemoPriceAndIssueLoanTransaction(
  connection: Connection,
  config: ClusterConfig,
  payer: PublicKey,
  params: DemoPriceAndIssueLoanParams,
) {
  const lendingProgram = lendingConsumerProgramId(config);
  const borrower = params.borrower ?? payer;
  const memo =
    `Halcyon one-tx demo; preview_quote + price_note + issue_loan; ` +
    `receipt ${params.receiptMint.toBase58()}; loan ${params.loanId.toString()}`;
  const pricingInstruction = previewInstruction("flagship", config, params.notionalBaseUnits);
  const priceNoteInstruction = new TransactionInstruction({
    programId: lendingProgram,
    keys: [
      accountMeta(payer, true, true),
      accountMeta(params.receiptMint),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data: instructionCoders.lending.encode("price_note", {
      args: {
        fair_value_usdc: params.fairValueBaseUnits,
        lending_value_usdc: params.lendingValueBaseUnits,
        max_borrow_usdc: params.maxBorrowBaseUnits,
        source_slot: params.sourceSlot,
      },
    }),
  });
  const issueLoanInstruction = new TransactionInstruction({
    programId: lendingProgram,
    keys: [
      accountMeta(payer, true, true),
      accountMeta(borrower),
      accountMeta(params.receiptMint),
      accountMeta(SystemProgram.programId),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data: instructionCoders.lending.encode("issue_loan", {
      args: {
        loan_id: params.loanId,
        principal_usdc: params.notionalBaseUnits,
        lending_value_usdc: params.lendingValueBaseUnits,
        debt_usdc: params.debtBaseUnits,
      },
    }),
  });

  const instructions = [pricingInstruction, priceNoteInstruction, issueLoanInstruction];
  if (params.includeMemo) {
    instructions.push(
      new TransactionInstruction({
        programId: MEMO_PROGRAM_ID,
        keys: [],
        data: Buffer.from(memo.slice(0, 512), "utf8"),
      }),
    );
  }

  return buildSingleInstructionTransaction(connection, payer, instructions);
}

export async function buildMockLendingBorrowTransaction(
  connection: Connection,
  config: ClusterConfig,
  payer: PublicKey,
  markerRecipient: PublicKey,
  memo: string,
  includeMemo: boolean,
  pricing: MockLendingBorrowPricing,
) {
  const pricingInstruction =
    pricing.mode === "flagshipLendingValue"
      ? flagshipLendingValueInstruction(config, pricing.policyAddress, pricing.productTermsAddress)
      : previewInstruction("flagship", config, pricing.notionalBaseUnits);
  const instructions = [
    pricingInstruction,
    SystemProgram.transfer({
      fromPubkey: payer,
      toPubkey: markerRecipient,
      lamports: 1,
    }),
  ];

  if (includeMemo) {
    instructions.push(
      new TransactionInstruction({
        programId: MEMO_PROGRAM_ID,
        keys: [],
        data: Buffer.from(memo.slice(0, 512), "utf8"),
      }),
    );
  }

  return buildSingleInstructionTransaction(connection, payer, instructions);
}

async function buildSingleInstructionTransaction(
  connection: Connection,
  payer: PublicKey,
  instructions: TransactionInstruction[],
) {
  const latestBlockhash = await connection.getLatestBlockhash(CONFIRMED);
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: latestBlockhash.blockhash,
    instructions: withComputeBudget(instructions),
  }).compileToV0Message([]);
  return new VersionedTransaction(message);
}

async function loadLookupTables(connection: Connection, kernelId: PublicKey, productId: PublicKey) {
  const registryAddress = altRegistryAddress(kernelId, productId);
  const registryInfo = await connection.getAccountInfo(registryAddress, CONFIRMED);
  if (!registryInfo?.data) {
    throw new Error("LookupTableRegistry account not found for this product");
  }
  const decoded = decodeKernelAccount("LookupTableRegistry", registryInfo.data);
  const tables = (field<unknown[]>(decoded, "tables") ?? [])
    .map((value) => toStringValue(value))
    .filter((value) => value && value !== PublicKey.default.toBase58());

  if (!tables.length) {
    throw new Error("No address lookup tables are registered for this product");
  }

  const resolved = await Promise.all(
    tables.map(async (address) => {
      const response = await connection.getAddressLookupTable(new PublicKey(address), {
        commitment: CONFIRMED,
      });
      return response.value;
    }),
  );

  const lookupTables = resolved.filter(Boolean) as AddressLookupTableAccount[];
  if (!lookupTables.length) {
    throw new Error("Lookup tables are registered but not readable from the selected RPC endpoint");
  }
  return lookupTables;
}

function accountDiscriminator(name: string) {
  const account = (kernelIdl.accounts ?? []).find((candidate) => candidate.name === name);
  if (!account?.discriminator) {
    throw new Error(`Missing account discriminator for ${name}`);
  }
  return Buffer.from(account.discriminator);
}

async function fetchPolicyHeadersForProduct(
  connection: Connection,
  config: ClusterConfig,
  kind: ProductKind,
  owner?: PublicKey,
) {
  const kernelId = kernelProgramId(config);
  const productId = programIdForKind(kind, config);
  const filters: GetProgramAccountsFilter[] = [
    { memcmp: { offset: 0, bytes: bs58.encode(accountDiscriminator("PolicyHeader")) } },
    { memcmp: { offset: 9, bytes: productId.toBase58() } },
  ];
  if (owner) {
    filters.push({ memcmp: { offset: 41, bytes: owner.toBase58() } });
  }

  const accounts = await connection.getProgramAccounts(kernelId, {
    commitment: CONFIRMED,
    filters,
  });

  return accounts.map(({ pubkey, account }) => ({
    pubkey,
    decoded: decodeKernelAccount("PolicyHeader", account.data),
  }));
}

export async function fetchPortfolio(connection: Connection, config: ClusterConfig, owner: PublicKey) {
  const productKinds: ProductKind[] = ["flagship", "solAutocall", "ilProtection"];
  const groups = await Promise.all(productKinds.map((kind) => fetchPolicyHeadersForProduct(connection, config, kind, owner)));
  const results: PortfolioEntry[] = [];

  for (const [index, headers] of groups.entries()) {
    const kind = productKinds[index];
    const termAddressStrings = headers
      .map(({ decoded }) => toStringValue(field(decoded, "productTerms")))
      .filter(Boolean);
    const termsAddresses = termAddressStrings.map((value) => new PublicKey(value));
    const termInfos = termsAddresses.length
      ? await connection.getMultipleAccountsInfo(termsAddresses, CONFIRMED)
      : [];
    const termInfoByAddress = new Map(
      termsAddresses.map((address, termIndex) => [address.toBase58(), termInfos[termIndex]]),
    );

    for (const { pubkey, decoded } of headers) {
      const productTermsAddress = toStringValue(field(decoded, "productTerms"));
      const termInfo = productTermsAddress ? termInfoByAddress.get(productTermsAddress) : null;
      const status = enumTag(field(decoded, "status"));
      const terms = termInfo?.data ? (decodeProductTerms(kind, termInfo.data as Buffer) as Record<string, unknown>) : null;

      let details: Record<string, string>;
      if (kind === "flagship") {
        let lendingValue: Record<string, unknown> | null = null;
        if (status.toLowerCase() === "active" && productTermsAddress) {
          lendingValue = await simulateFlagshipLendingValue(
            connection,
            config,
            owner,
            pubkey,
            new PublicKey(productTermsAddress),
          ).catch(() => null);
        }
        details = {
          "Lending value": lendingValue
            ? formatUsdcRaw(field(lendingValue, "lendingValuePayoutUsdc"))
            : "Unavailable",
          NAV: lendingValue ? formatScale6Percent(field(lendingValue, "navS6")) : "Unavailable",
          "KI level": lendingValue ? formatScale6Percent(field(lendingValue, "kiLevelUsdS6")) : "Unavailable",
          "Offered coupon": formatMaybePercent(field(terms ?? {}, "offeredCouponBpsS6"), 4),
          "Next monthly coupon": String(toNumber(field(terms ?? {}, "nextCouponIndex")) + 1),
          "Next autocall check": String(toNumber(field(terms ?? {}, "nextAutocallIndex")) + 1),
          "KI latched": booleanLabel(field(terms ?? {}, "kiLatched")),
        };
      } else if (kind === "ilProtection") {
        details = {
          "Pricing sigma": formatMaybePercent(field(terms ?? {}, "sigmaPricingS6")),
          Regime: enumTag(field(terms ?? {}, "regime")),
          Deductible: formatMaybePercent(field(terms ?? {}, "deductibleS6")),
          Cap: formatMaybePercent(field(terms ?? {}, "capS6")),
        };
      } else {
        details = {
          "Offered coupon": formatMaybePercent(field(terms ?? {}, "offeredCouponBpsS6"), 182.5),
          "Observation index": String(toNumber(field(terms ?? {}, "currentObservationIndex")) + 1),
          "KI triggered": booleanLabel(field(terms ?? {}, "kiTriggered")),
          "Coupons paid": formatUsdcRaw(field(terms ?? {}, "accumulatedCouponUsdc")),
        };
      }

      results.push({
        policyAddress: pubkey.toBase58(),
        productKind: kind,
        owner: owner.toBase58(),
        status,
        notional: toNumber(field(decoded, "notional")),
        premiumPaid: toNumber(field(decoded, "premiumPaid")),
        maxLiability: toNumber(field(decoded, "maxLiability")),
        issuedAt: toNumber(field(decoded, "issuedAt")),
        expiryTs: toNumber(field(decoded, "expiryTs")),
        productTermsAddress: toStringValue(field(decoded, "productTerms")),
        details,
      });
    }
  }

  return results.sort((left, right) => right.issuedAt - left.issuedAt);
}

export async function fetchVaultOverview(connection: Connection, config: ClusterConfig) {
  const kernelId = kernelProgramId(config);
  const [protocolInfo, vaultInfo, feeInfo, keeperInfo] = await connection.getMultipleAccountsInfo(
    [
      protocolConfigAddress(kernelId),
      vaultStateAddress(kernelId),
      feeLedgerAddress(kernelId),
      keeperRegistryAddress(kernelId),
    ],
    CONFIRMED,
  );

  if (!protocolInfo?.data || !vaultInfo?.data) {
    throw new Error("Kernel accounts are not initialized on this cluster");
  }

  const protocolConfig = decodeKernelAccount("ProtocolConfig", protocolInfo.data);
  const vaultState = decodeKernelAccount("VaultState", vaultInfo.data);
  const feeLedger = feeInfo?.data ? decodeKernelAccount("FeeLedger", feeInfo.data) : null;
  const keeperRegistry = keeperInfo?.data ? decodeKernelAccount("KeeperRegistry", keeperInfo.data) : null;

  const kinds: ProductKind[] = ["flagship", "solAutocall", "ilProtection"];
  const productSummaries = await Promise.all(
    kinds.map(async (kind) => {
      const productId = programIdForKind(kind, config);
      const registryInfo = await connection.getAccountInfo(productRegistryAddress(kernelId, productId), CONFIRMED);
      if (!registryInfo?.data) {
        throw new Error(`Missing ProductRegistryEntry for ${kind}`);
      }

      const registry = decodeKernelAccount("ProductRegistryEntry", registryInfo.data);
      const policies = await fetchPolicyHeadersForProduct(connection, config, kind);
      const activePolicyCount = policies.filter(
        ({ decoded }) => enumTag(field(decoded, "status")).toLowerCase() === "active",
      ).length;
      const settledPolicyCount = policies.filter(
        ({ decoded }) => enumTag(field(decoded, "status")).toLowerCase() === "settled",
      ).length;

      const couponVaultInfo = await connection.getAccountInfo(couponVaultAddress(kernelId, productId), CONFIRMED);
      const couponVaultBalance = couponVaultInfo?.data
        ? toNumber(field(decodeKernelAccount("CouponVault", couponVaultInfo.data), "usdcBalance"))
        : null;

      const hedgeInfo = await connection.getAccountInfo(hedgeSleeveAddress(kernelId, productId), CONFIRMED);
      const hedgeReserve = hedgeInfo?.data
        ? toNumber(field(decodeKernelAccount("HedgeSleeve", hedgeInfo.data), "usdcReserve"))
        : null;

      return {
        kind,
        registry,
        activePolicyCount,
        settledPolicyCount,
        couponVaultBalance,
        hedgeReserve,
      };
    }),
  );

  return {
    protocolConfig,
    vaultState,
    feeLedger,
    keeperRegistry,
    productSummaries,
  } satisfies VaultOverview;
}

function formatMaybePercent(value: unknown, multiplier = 1) {
  const numeric = toNumber(value);
  if (!numeric) return "0.00%";
  return `${((numeric / 1_000_000 / 10_000) * 100 * multiplier).toFixed(multiplier > 10 ? 1 : 2)}%`;
}

function formatUsdcRaw(value: unknown) {
  return `$${(toNumber(value) / 1_000_000).toFixed(2)}`;
}

function formatScale6Percent(value: unknown) {
  return `${((toNumber(value) / 1_000_000) * 100).toFixed(1)}%`;
}

function booleanLabel(value: unknown) {
  return value ? "Yes" : "No";
}
