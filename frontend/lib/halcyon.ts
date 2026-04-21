import { Buffer } from "buffer";
import { BorshAccountsCoder, BorshCoder, BorshInstructionCoder, BN, type Idl } from "@coral-xyz/anchor";
import {
  AddressLookupTableAccount,
  Connection,
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
const solIdl = solIdlJson as Idl;

const coders = {
  flagship: new BorshCoder(flagshipIdl),
  il: new BorshCoder(ilIdl),
  kernel: new BorshCoder(kernelIdl),
  sol: new BorshCoder(solIdl),
};

const instructionCoders = {
  flagship: new BorshInstructionCoder(flagshipIdl),
  il: new BorshInstructionCoder(ilIdl),
  sol: new BorshInstructionCoder(solIdl),
};

const accountCoders = {
  flagship: new BorshAccountsCoder(flagshipIdl),
  il: new BorshAccountsCoder(ilIdl),
  kernel: new BorshAccountsCoder(kernelIdl),
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
  altRegistry: Buffer.from("alt_registry"),
  couponVault: Buffer.from("coupon_vault"),
  keeperRegistry: Buffer.from("keeper_registry"),
  hedgeSleeve: Buffer.from("hedge_sleeve"),
};

const CONFIRMED: Commitment = "confirmed";
const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
);

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

function policyAddress(kernelProgramId: PublicKey, policyId: PublicKey) {
  return PublicKey.findProgramAddressSync([SEEDS.policy, policyId.toBuffer()], kernelProgramId)[0];
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

function decodeReturnData(kind: ProductKind, returnData: { programId: string; data: [string, string] }) {
  const bytes = Buffer.from(returnData.data[0], "base64");
  const coder =
    kind === "flagship" ? coders.flagship : kind === "ilProtection" ? coders.il : coders.sol;
  return coder.types.decode("QuotePreview", bytes) as Record<string, unknown>;
}

function decodeKernelAccount(name: string, data: Buffer) {
  return accountCoders.kernel.decode(name, data) as Record<string, unknown>;
}

function decodeProductTerms(kind: ProductKind, data: Buffer) {
  if (kind === "flagship") return accountCoders.flagship.decode("FlagshipAutocallTerms", data);
  if (kind === "ilProtection") return accountCoders.il.decode("IlProtectionTerms", data);
  return accountCoders.sol.decode("SolAutocallTerms", data);
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
      accountMeta(feeds.pythSol),
      accountMeta(SYSVAR_CLOCK_PUBKEY),
    ],
    data,
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
  const treasuryDestination = field(protocolConfig, "treasuryDestination");
  if (!treasuryDestination) {
    throw new Error("ProtocolConfig treasury destination is missing");
  }
  const treasuryAccount = await connection.getAccountInfo(new PublicKey(toStringValue(treasuryDestination)), CONFIRMED);
  if (!treasuryAccount?.data?.length || treasuryAccount.data.length < 32) {
    throw new Error("Treasury destination is not a token account");
  }
  const usdcMint = new PublicKey(treasuryAccount.data.slice(0, 32));

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
  const payer = Keypair.generate().publicKey;
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: blockhash.blockhash,
    instructions: [ix],
  }).compileToV0Message([]);
  const tx = new VersionedTransaction(message);
  const result = await connection.simulateTransaction(tx, {
    sigVerify: false,
    replaceRecentBlockhash: true,
    commitment: CONFIRMED,
  });
  if (result.value.err) {
    throw new Error(`Preview failed: ${JSON.stringify(result.value.err)}`);
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
    instructions: [instruction],
  }).compileToV0Message(lookupTables);

  return {
    policyId,
    transaction: new VersionedTransaction(message),
  };
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
    const termsAddresses = headers
      .map(({ decoded }) => field(decoded, "productTerms"))
      .filter(Boolean)
      .map((value) => new PublicKey(toStringValue(value)));
    const termInfos = termsAddresses.length
      ? await connection.getMultipleAccountsInfo(termsAddresses, CONFIRMED)
      : [];

    headers.forEach(({ pubkey, decoded }, headerIndex) => {
      const termInfo = termInfos[headerIndex];
      const status = enumTag(field(decoded, "status"));
      const terms = termInfo?.data ? (decodeProductTerms(kind, termInfo.data as Buffer) as Record<string, unknown>) : null;

      let details: Record<string, string>;
      if (kind === "flagship") {
        details = {
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
    });
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

function booleanLabel(value: unknown) {
  return value ? "Yes" : "No";
}
