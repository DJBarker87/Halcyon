import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import {
  AddressLookupTableAccount,
  AddressLookupTableProgram,
  Ed25519Program,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SYSVAR_INSTRUCTIONS_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SystemProgram,
  TransactionMessage,
  VersionedTransaction,
} from "@solana/web3.js";
import { assert } from "chai";
import { createHash } from "crypto";
import path from "path";

import { HalcyonFlagshipAutocall } from "../../target/types/halcyon_flagship_autocall";
import { HalcyonIlProtection } from "../../target/types/halcyon_il_protection";
import { HalcyonKernel } from "../../target/types/halcyon_kernel";
import { HalcyonSolAutocall } from "../../target/types/halcyon_sol_autocall";
import {
  createMint,
  getAssociatedTokenAddressSync,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  TOKEN_PROGRAM_ID,
} from "../kernel/token_harness";
import {
  loadMockOracleManifest,
  MockOracleFixture,
  MockOracleManifest,
} from "./mock_pyth";

const SEEDS = {
  protocolConfig: Buffer.from("protocol_config"),
  vaultState: Buffer.from("vault_state"),
  feeLedger: Buffer.from("fee_ledger"),
  keeperRegistry: Buffer.from("keeper_registry"),
  productRegistry: Buffer.from("product_registry"),
  policy: Buffer.from("policy"),
  terms: Buffer.from("terms"),
  vaultSigma: Buffer.from("vault_sigma"),
  regimeSignal: Buffer.from("regime_signal"),
  regression: Buffer.from("regression"),
  aggregateDelta: Buffer.from("aggregate_delta"),
  couponVault: Buffer.from("coupon_vault"),
  hedgeSleeve: Buffer.from("hedge_sleeve"),
  hedgeBook: Buffer.from("hedge_book"),
  vaultAuthority: Buffer.from("vault_authority"),
  vaultUsdc: Buffer.from("vault_usdc"),
  treasuryUsdc: Buffer.from("treasury_usdc"),
  productAuthority: Buffer.from("product_authority"),
  senior: Buffer.from("senior"),
  junior: Buffer.from("junior"),
  altRegistry: Buffer.from("alt_registry"),
} as const;

export const KEEPER_ROLE = {
  observation: 0,
  regression: 1,
  delta: 2,
  hedge: 3,
  regime: 4,
} as const;

const DEFAULT_MANIFEST_PATH = path.resolve(
  process.cwd(),
  ".anchor/integration/mock_pyth_manifest.json"
);

export type KernelPrograms = {
  flagshipAutocall: Program<HalcyonFlagshipAutocall>;
  ilProtection: Program<HalcyonIlProtection>;
  kernel: Program<HalcyonKernel>;
  solAutocall: Program<HalcyonSolAutocall>;
};

export type ProductAccounts = {
  aggregateDelta?: PublicKey;
  authority: PublicKey;
  couponVault: PublicKey;
  couponVaultUsdc: PublicKey;
  hedgeBook?: PublicKey;
  hedgeSleeve?: PublicKey;
  hedgeSleeveUsdc?: PublicKey;
  lookupTable: PublicKey;
  lookupTableRegistry: PublicKey;
  productRegistryEntry: PublicKey;
  regimeSignal?: PublicKey;
  regression?: PublicKey;
  vaultSigma: PublicKey;
};

export type BuyerContext = {
  keypair: Keypair;
  usdc: PublicKey;
};

export type TestContext = {
  admin: anchor.Wallet;
  adminUsdc: PublicKey;
  buyers: BuyerContext[];
  depositors: {
    juniorUsdc: PublicKey;
    senior: Keypair;
    seniorUsdc: PublicKey;
  };
  keepers: Record<keyof typeof KEEPER_ROLE, Keypair>;
  manifest: MockOracleManifest;
  oracles: Record<string, MockOracleFixture>;
  pdas: {
    feeLedger: PublicKey;
    keeperRegistry: PublicKey;
    protocolConfig: PublicKey;
    treasuryUsdc: PublicKey;
    vaultAuthority: PublicKey;
    vaultState: PublicKey;
    vaultUsdc: PublicKey;
  };
  products: {
    flagship: ProductAccounts;
    il: ProductAccounts;
    sol: ProductAccounts;
  };
  programs: KernelPrograms;
  provider: anchor.AnchorProvider;
  usdcMint: PublicKey;
};

function pda(
  seeds: Buffer[],
  programId: PublicKey
): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, programId)[0];
}

export function wrongProgramDerivedPda(
  seeds: Buffer[],
  wrongProgramId: PublicKey
): PublicKey {
  return pda(seeds, wrongProgramId);
}

export function accountDiscriminator(name: string): number[] {
  return Array.from(
    createHash("sha256").update(`account:${name}`).digest().subarray(0, 8)
  );
}

function manifestPath(): string {
  return process.env.HALCYON_MOCK_PYTH_MANIFEST ?? DEFAULT_MANIFEST_PATH;
}

async function airdrop(
  provider: anchor.AnchorProvider,
  recipient: PublicKey,
  sol: number
): Promise<void> {
  const sig = await provider.connection.requestAirdrop(
    recipient,
    sol * LAMPORTS_PER_SOL
  );
  await provider.connection.confirmTransaction(sig, "confirmed");
}

async function waitForLookupTableReady(
  provider: anchor.AnchorProvider,
  lookupTable: PublicKey,
  minimumAddresses: number
): Promise<AddressLookupTableAccount> {
  for (let i = 0; i < 120; i += 1) {
    const lookupAccount = (
      await provider.connection.getAddressLookupTable(lookupTable, {
        commitment: "confirmed",
      })
    ).value;
    if (
      lookupAccount &&
      lookupAccount.state.addresses.length >= minimumAddresses
    ) {
      return lookupAccount;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`ALT ${lookupTable.toBase58()} did not resolve`);
}

function fixedBytes(input: string, size: number): number[] {
  const out = Buffer.alloc(size);
  Buffer.from(input, "utf8").copy(out, 0, 0, size);
  return Array.from(out);
}

const AGGREGATE_DELTA_DOMAIN_TAG = Buffer.from(
  "halcyon-aggregate-delta-v1\n",
  "utf8"
);

function encodeAggregateDeltaMessage(
  merkleRoot: number[],
  pythPublishTimes: BN[],
  spotSnapshotS6: BN[],
  sequence: number,
  productProgramId: PublicKey
): Buffer {
  const out = Buffer.alloc(147);
  AGGREGATE_DELTA_DOMAIN_TAG.copy(out, 0);
  Buffer.from(merkleRoot).copy(out, 27);

  pythPublishTimes.forEach((value, index) => {
    out.writeBigInt64LE(
      BigInt(value.toString()),
      59 + index * 8
    );
  });
  spotSnapshotS6.forEach((value, index) => {
    out.writeBigInt64LE(
      BigInt(value.toString()),
      83 + index * 8
    );
  });
  out.writeBigUInt64LE(BigInt(sequence), 107);
  productProgramId.toBuffer().copy(out, 115);
  return out;
}

export async function writeAggregateDelta(
  provider: anchor.AnchorProvider,
  kernel: Program<HalcyonKernel>,
  deltaKeeper: Keypair,
  accounts: {
    aggregateDelta: PublicKey;
    keeperRegistry: PublicKey;
    payer: PublicKey;
    productRegistryEntry: PublicKey;
    protocolConfig: PublicKey;
    systemProgram: PublicKey;
  },
  args: {
    deltaSpyS6: BN;
    deltaQqqS6: BN;
    deltaIwmS6: BN;
    liveNoteCount: number;
    merkleRoot: number[];
    productProgramId: PublicKey;
    publicationCid: number[];
    pythPublishTimes: BN[];
    spotIwmS6: BN;
    spotQqqS6: BN;
    spotSpyS6: BN;
  }
): Promise<string> {
  const aggregateDeltaAccount = await provider.connection.getAccountInfo(
    accounts.aggregateDelta,
    "confirmed"
  );
  const sequence = aggregateDeltaAccount
    ? aggregateDeltaAccount.data.readBigUInt64LE(165) + 1n
    : 1n;
  const signedMessage = encodeAggregateDeltaMessage(
    args.merkleRoot,
    args.pythPublishTimes,
    [args.spotSpyS6, args.spotQqqS6, args.spotIwmS6],
    Number(sequence),
    args.productProgramId
  );

  const ed25519Ix = Ed25519Program.createInstructionWithPrivateKey({
    message: signedMessage,
    privateKey: deltaKeeper.secretKey.subarray(0, 32),
  });
  const writeIx = await kernel.methods
    .writeAggregateDelta({
      deltaSpyS6: args.deltaSpyS6,
      deltaQqqS6: args.deltaQqqS6,
      deltaIwmS6: args.deltaIwmS6,
      liveNoteCount: args.liveNoteCount,
      merkleRoot: args.merkleRoot,
      productProgramId: args.productProgramId,
      publicationCid: args.publicationCid,
      pythPublishTimes: args.pythPublishTimes,
      spotIwmS6: args.spotIwmS6,
      spotQqqS6: args.spotQqqS6,
      spotSpyS6: args.spotSpyS6,
    })
    .accounts({
      aggregateDelta: accounts.aggregateDelta,
      instructionsSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
      keeper: deltaKeeper.publicKey,
      keeperRegistry: accounts.keeperRegistry,
      payer: accounts.payer,
      productRegistryEntry: accounts.productRegistryEntry,
      protocolConfig: accounts.protocolConfig,
      systemProgram: accounts.systemProgram,
    } as any)
    .instruction();

  const tx = new anchor.web3.Transaction().add(ed25519Ix, writeIx);
  return provider.sendAndConfirm(tx, [deltaKeeper]);
}

async function createAndRegisterLookupTable(
  provider: anchor.AnchorProvider,
  kernel: Program<HalcyonKernel>,
  admin: anchor.Wallet,
  protocolConfig: PublicKey,
  productProgramId: PublicKey,
  addresses: PublicKey[]
): Promise<{ lookupTable: PublicKey; lookupTableRegistry: PublicKey }> {
  const slot = await provider.connection.getSlot("finalized");
  const [createIx, lookupTable] = AddressLookupTableProgram.createLookupTable({
    authority: admin.publicKey,
    payer: admin.publicKey,
    recentSlot: slot,
  });

  const extendIx = AddressLookupTableProgram.extendLookupTable({
    payer: admin.publicKey,
    authority: admin.publicKey,
    lookupTable,
    addresses,
  });

  const recent = (await provider.connection.getLatestBlockhash()).blockhash;
  const message = new TransactionMessage({
    instructions: [createIx, extendIx],
    payerKey: admin.publicKey,
    recentBlockhash: recent,
  }).compileToV0Message();

  const tx = new VersionedTransaction(message);
  const signed = await provider.wallet.signTransaction(tx);
  const sig = await provider.connection.sendTransaction(signed);
  await provider.connection.confirmTransaction(sig, "confirmed");

  const lookupAccount = await waitForLookupTableReady(
    provider,
    lookupTable,
    addresses.length
  );
  const createdSlot = await provider.connection.getSlot("confirmed");
  for (let i = 0; i < 60; i += 1) {
    const currentSlot = await provider.connection.getSlot("confirmed");
    if (currentSlot > createdSlot) {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  const lookupTableRegistry = pda(
    [SEEDS.altRegistry, productProgramId.toBuffer()],
    kernel.programId
  );

  await kernel.methods
    .registerLookupTable(lookupTable)
    .accounts({
      admin: admin.publicKey,
      lookupTableAccount: lookupTable,
      lookupTableRegistry,
      productProgramId,
      protocolConfig,
      systemProgram: SystemProgram.programId,
    } as any)
    .rpc();

  assert(
    lookupAccount.state.addresses.length >= addresses.length,
    "lookup table did not contain the expected addresses"
  );

  return { lookupTable, lookupTableRegistry };
}

export function deriveCommonPdas(
  kernelProgramId: PublicKey,
  usdcMint: PublicKey
): TestContext["pdas"] {
  return {
    feeLedger: pda([SEEDS.feeLedger], kernelProgramId),
    keeperRegistry: pda([SEEDS.keeperRegistry], kernelProgramId),
    protocolConfig: pda([SEEDS.protocolConfig], kernelProgramId),
    treasuryUsdc: pda([SEEDS.treasuryUsdc, usdcMint.toBuffer()], kernelProgramId),
    vaultAuthority: pda([SEEDS.vaultAuthority], kernelProgramId),
    vaultState: pda([SEEDS.vaultState], kernelProgramId),
    vaultUsdc: pda([SEEDS.vaultUsdc, usdcMint.toBuffer()], kernelProgramId),
  };
}

function deriveProductAccounts(
  kernelProgramId: PublicKey,
  productProgramId: PublicKey,
  usdcMint: PublicKey
): ProductAccounts {
  const couponVault = pda(
    [SEEDS.couponVault, productProgramId.toBuffer()],
    kernelProgramId
  );
  const hedgeSleeve = pda(
    [SEEDS.hedgeSleeve, productProgramId.toBuffer()],
    kernelProgramId
  );

  return {
    aggregateDelta: pda(
      [SEEDS.aggregateDelta, productProgramId.toBuffer()],
      kernelProgramId
    ),
    authority: pda([SEEDS.productAuthority], productProgramId),
    couponVault,
    couponVaultUsdc: getAssociatedTokenAddressSync(usdcMint, couponVault),
    hedgeBook: pda([SEEDS.hedgeBook, productProgramId.toBuffer()], kernelProgramId),
    hedgeSleeve,
    hedgeSleeveUsdc: getAssociatedTokenAddressSync(usdcMint, hedgeSleeve),
    lookupTable: PublicKey.default,
    lookupTableRegistry: pda(
      [SEEDS.altRegistry, productProgramId.toBuffer()],
      kernelProgramId
    ),
    productRegistryEntry: pda(
      [SEEDS.productRegistry, productProgramId.toBuffer()],
      kernelProgramId
    ),
    regimeSignal: pda(
      [SEEDS.regimeSignal, productProgramId.toBuffer()],
      kernelProgramId
    ),
    regression: pda([SEEDS.regression], kernelProgramId),
    vaultSigma: pda(
      [SEEDS.vaultSigma, productProgramId.toBuffer()],
      kernelProgramId
    ),
  };
}

export async function setupFullProtocol(): Promise<TestContext> {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const programs: KernelPrograms = {
    flagshipAutocall:
      anchor.workspace
        .halcyonFlagshipAutocall as Program<HalcyonFlagshipAutocall>,
    ilProtection:
      anchor.workspace.halcyonIlProtection as Program<HalcyonIlProtection>,
    kernel: anchor.workspace.halcyonKernel as Program<HalcyonKernel>,
    solAutocall:
      anchor.workspace.halcyonSolAutocall as Program<HalcyonSolAutocall>,
  };

  const admin = provider.wallet as anchor.Wallet;
  const manifest = loadMockOracleManifest(manifestPath());
  const oracles = manifest.fixtures;

  const seniorDepositor = Keypair.generate();
  const buyers = [Keypair.generate(), Keypair.generate()];
  const keepers = {
    delta: Keypair.generate(),
    hedge: Keypair.generate(),
    observation: Keypair.generate(),
    regression: Keypair.generate(),
    regime: Keypair.generate(),
  };

  for (const signer of [
    seniorDepositor,
    ...buyers,
    keepers.observation,
    keepers.regression,
    keepers.delta,
    keepers.hedge,
    keepers.regime,
  ]) {
    await airdrop(provider, signer.publicKey, 2);
  }

  const usdcMint = await createMint(
    provider.connection,
    admin.payer,
    admin.publicKey,
    null,
    6
  );
  const pdas = deriveCommonPdas(programs.kernel.programId, usdcMint);

  const adminUsdc = (
    await getOrCreateAssociatedTokenAccount(
      provider.connection,
      admin.payer,
      usdcMint,
      admin.publicKey
    )
  ).address;
  const seniorUsdc = (
    await getOrCreateAssociatedTokenAccount(
      provider.connection,
      admin.payer,
      usdcMint,
      seniorDepositor.publicKey
    )
  ).address;
  const juniorUsdc = adminUsdc;

  const fundedBuyers: BuyerContext[] = [];
  for (const buyer of buyers) {
    const buyerUsdc = (
      await getOrCreateAssociatedTokenAccount(
        provider.connection,
        admin.payer,
        usdcMint,
        buyer.publicKey
      )
    ).address;
    fundedBuyers.push({ keypair: buyer, usdc: buyerUsdc });
  }

  await mintTo(
    provider.connection,
    admin.payer,
    usdcMint,
    adminUsdc,
    admin.payer,
    2_000_000_000_000n
  );
  await mintTo(
    provider.connection,
    admin.payer,
    usdcMint,
    seniorUsdc,
    admin.payer,
    500_000_000_000n
  );
  for (const buyer of fundedBuyers) {
    await mintTo(
      provider.connection,
      admin.payer,
      usdcMint,
      buyer.usdc,
      admin.payer,
      500_000_000_000n
    );
  }

  await programs.kernel.methods
    .initializeProtocol({
      utilizationCapBps: new BN(10_000),
      seniorShareBps: 9_000,
      juniorShareBps: 300,
      treasuryShareBps: 700,
      seniorCooldownSecs: new BN(0),
      ewmaRateLimitSecs: new BN(1),
      sigmaStalenessCapSecs: new BN(600),
      regimeStalenessCapSecs: new BN(600),
      regressionStalenessCapSecs: new BN(600),
      pythQuoteStalenessCapSecs: new BN(600),
      pythSettleStalenessCapSecs: new BN(600),
      quoteTtlSecs: new BN(2),
      sigmaFloorAnnualisedS6: new BN(400_000),
      sigmaCeilingAnnualisedS6: new BN(800_000),
      solAutocallQuoteShareBps: 7_500,
      solAutocallIssuerMarginBps: 50,
      treasuryDestination: adminUsdc,
      hedgeMaxSlippageBpsCap: 100,
      hedgeDefundDestination: adminUsdc,
    })
    .accounts({
      admin: admin.publicKey,
      feeLedger: pdas.feeLedger,
      keeperRegistry: pdas.keeperRegistry,
      protocolConfig: pdas.protocolConfig,
      rent: SYSVAR_RENT_PUBKEY,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      treasuryUsdc: pdas.treasuryUsdc,
      usdcMint,
      vaultAuthority: pdas.vaultAuthority,
      vaultState: pdas.vaultState,
      vaultUsdc: pdas.vaultUsdc,
    } as any)
    .rpc();

  await programs.kernel.methods
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
      solAutocallSigmaFloorAnnualisedS6: null,
      flagshipSigmaFloorAnnualisedS6: null,
      sigmaCeilingAnnualisedS6: null,
      k12CorrectionSha256: Array.from(
        Buffer.from(
          "b5faa897dddd970105589a82c5b5a3404c561e51c500ebc56d948a19c8b2ea6e",
          "hex"
        )
      ),
      dailyKiCorrectionSha256: Array.from(
        Buffer.from(
          "f89ac13789dafe2f933d473799f9b617b666d2ac7682c90bc50f59862e61ee0f",
          "hex"
        )
      ),
      podDeimTableSha256: null,
      premiumSplitsBps: null,
      solAutocallQuoteConfigBps: null,
      treasuryDestination: null,
      hedgeMaxSlippageBpsCap: null,
      hedgeDefundDestination: null,
    })
    .accounts({
      admin: admin.publicKey,
      protocolConfig: pdas.protocolConfig,
    } as any)
    .rpc();

  await programs.kernel.methods
    .depositSenior(new BN(500_000_000_000))
    .accounts({
      depositor: seniorDepositor.publicKey,
      depositorUsdc: seniorUsdc,
      protocolConfig: pdas.protocolConfig,
      seniorDeposit: pda(
        [SEEDS.senior, seniorDepositor.publicKey.toBuffer()],
        programs.kernel.programId
      ),
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      usdcMint,
      vaultState: pdas.vaultState,
      vaultUsdc: pdas.vaultUsdc,
    } as any)
    .signers([seniorDepositor])
    .rpc();

  await programs.kernel.methods
    .seedJunior(new BN(250_000_000_000))
    .accounts({
      admin: admin.publicKey,
      adminUsdc,
      junior: pda(
        [SEEDS.junior, admin.publicKey.toBuffer()],
        programs.kernel.programId
      ),
      protocolConfig: pdas.protocolConfig,
      tokenProgram: TOKEN_PROGRAM_ID,
      usdcMint,
      vaultState: pdas.vaultState,
      vaultUsdc: pdas.vaultUsdc,
    } as any)
    .rpc();

  for (const [role, value] of Object.entries(KEEPER_ROLE)) {
    await programs.kernel.methods
      .rotateKeeper(value, keepers[role as keyof typeof KEEPER_ROLE].publicKey)
      .accounts({
        admin: admin.publicKey,
        keeperRegistry: pdas.keeperRegistry,
        protocolConfig: pdas.protocolConfig,
      } as any)
      .rpc();
  }

  const sol = deriveProductAccounts(
    programs.kernel.programId,
    programs.solAutocall.programId,
    usdcMint
  );
  const il = deriveProductAccounts(
    programs.kernel.programId,
    programs.ilProtection.programId,
    usdcMint
  );
  const flagship = deriveProductAccounts(
    programs.kernel.programId,
    programs.flagshipAutocall.programId,
    usdcMint
  );

  flagship.regimeSignal = undefined;
  il.regression = undefined;
  sol.regression = undefined;

  await programs.kernel.methods
    .registerProduct({
      productProgramId: programs.solAutocall.programId,
      expectedAuthority: sol.authority,
      oracleFeedId: Array.from(
        Buffer.from(
          "ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d",
          "hex"
        )
      ),
      perPolicyRiskCap: new BN(5_000_000_000_000),
      globalRiskCap: new BN(20_000_000_000_000),
      engineVersion: 1,
      initTermsDiscriminator: accountDiscriminator("SolAutocallTerms"),
      requiresPrincipalEscrow: true,
    })
    .accounts({
      admin: admin.publicKey,
      productRegistryEntry: sol.productRegistryEntry,
      protocolConfig: pdas.protocolConfig,
      systemProgram: SystemProgram.programId,
      vaultSigma: sol.vaultSigma,
    } as any)
    .rpc();

  await programs.kernel.methods
    .registerProduct({
      productProgramId: programs.ilProtection.programId,
      expectedAuthority: il.authority,
      oracleFeedId: Array.from(
        Buffer.from(
          "ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d",
          "hex"
        )
      ),
      perPolicyRiskCap: new BN(5_000_000_000_000),
      globalRiskCap: new BN(20_000_000_000_000),
      engineVersion: 1,
      initTermsDiscriminator: accountDiscriminator("IlProtectionTerms"),
      requiresPrincipalEscrow: false,
    })
    .accounts({
      admin: admin.publicKey,
      productRegistryEntry: il.productRegistryEntry,
      protocolConfig: pdas.protocolConfig,
      systemProgram: SystemProgram.programId,
      vaultSigma: il.vaultSigma,
    } as any)
    .rpc();

  await programs.kernel.methods
    .registerProduct({
      productProgramId: programs.flagshipAutocall.programId,
      expectedAuthority: flagship.authority,
      oracleFeedId: Array.from(
        Buffer.from(
          "19e09bb805456ada3979a7d1cbb4b6d63babc3a0f8e8a9509f68afa5c4c11cd5",
          "hex"
        )
      ),
      perPolicyRiskCap: new BN(5_000_000_000_000),
      globalRiskCap: new BN(20_000_000_000_000),
      engineVersion: 1,
      initTermsDiscriminator: accountDiscriminator("FlagshipAutocallTerms"),
      requiresPrincipalEscrow: true,
    })
    .accounts({
      admin: admin.publicKey,
      productRegistryEntry: flagship.productRegistryEntry,
      protocolConfig: pdas.protocolConfig,
      systemProgram: SystemProgram.programId,
      vaultSigma: flagship.vaultSigma,
    } as any)
    .rpc();

  for (const product of [sol, flagship]) {
    await programs.kernel.methods
      .fundCouponVault(
        product.authority.equals(sol.authority)
          ? programs.solAutocall.programId
          : programs.flagshipAutocall.programId,
        new BN(250_000_000_000)
      )
      .accounts({
        admin: admin.publicKey,
        adminUsdc,
        associatedTokenProgram:
          anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        couponVault: product.couponVault,
        couponVaultUsdc: product.couponVaultUsdc,
        productRegistryEntry: product.productRegistryEntry,
        protocolConfig: pdas.protocolConfig,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        usdcMint,
      } as any)
      .rpc();
  }

  for (const product of [sol, flagship]) {
    await programs.kernel.methods
      .fundHedgeSleeve(
        product.authority.equals(sol.authority)
          ? programs.solAutocall.programId
          : programs.flagshipAutocall.programId,
        new BN(100_000_000_000)
      )
      .accounts({
        admin: admin.publicKey,
        adminUsdc,
        associatedTokenProgram:
          anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        hedgeSleeve: product.hedgeSleeve,
        hedgeSleeveUsdc: product.hedgeSleeveUsdc,
        productRegistryEntry: product.productRegistryEntry,
        protocolConfig: pdas.protocolConfig,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        usdcMint,
      } as any)
      .rpc();
  }

  await programs.kernel.methods
    .updateEwma()
    .accounts({
      oraclePrice: new PublicKey(oracles["kernel-sol-init"].pubkey),
      protocolConfig: pdas.protocolConfig,
      vaultSigma: sol.vaultSigma,
    } as any)
    .rpc();
  await programs.kernel.methods
    .updateEwma()
    .accounts({
      oraclePrice: new PublicKey(oracles["kernel-sol-init"].pubkey),
      protocolConfig: pdas.protocolConfig,
      vaultSigma: il.vaultSigma,
    } as any)
    .rpc();
  await programs.kernel.methods
    .updateEwma()
    .accounts({
      oraclePrice: new PublicKey(oracles["kernel-spy-init"].pubkey),
      protocolConfig: pdas.protocolConfig,
      vaultSigma: flagship.vaultSigma,
    } as any)
    .rpc();

  for (const productProgramId of [
    programs.solAutocall.programId,
    programs.ilProtection.programId,
    programs.flagshipAutocall.programId,
  ]) {
    await programs.kernel.methods
      .writeRegimeSignal({
        productProgramId,
        fvolS6: new BN(250_000),
      })
      .accounts({
        keeper: keepers.regime.publicKey,
        keeperRegistry: pdas.keeperRegistry,
        payer: admin.publicKey,
        protocolConfig: pdas.protocolConfig,
        regimeSignal: pda(
          [SEEDS.regimeSignal, productProgramId.toBuffer()],
          programs.kernel.programId
        ),
        systemProgram: SystemProgram.programId,
      } as any)
      .signers([keepers.regime])
      .rpc();
  }

  await programs.kernel.methods
    .writeRegression({
      alphaS12: new BN(0),
      betaQqqS12: new BN(300_000_000_000),
      betaSpyS12: new BN(500_000_000_000),
      rSquaredS6: new BN(900_000),
      residualVolS6: new BN(120_000),
      sampleCount: 180,
      windowEndTs: new BN(manifest.baseTimestamp),
      windowStartTs: new BN(manifest.baseTimestamp - 86_400),
    })
    .accounts({
      keeper: keepers.regression.publicKey,
      keeperRegistry: pdas.keeperRegistry,
      payer: admin.publicKey,
      protocolConfig: pdas.protocolConfig,
      regression: flagship.regression,
      systemProgram: SystemProgram.programId,
    } as any)
    .signers([keepers.regression])
    .rpc();

  await writeAggregateDelta(
    provider,
    programs.kernel,
    keepers.delta,
    {
      aggregateDelta: flagship.aggregateDelta!,
      keeperRegistry: pdas.keeperRegistry,
      payer: admin.publicKey,
      productRegistryEntry: flagship.productRegistryEntry,
      protocolConfig: pdas.protocolConfig,
      systemProgram: SystemProgram.programId,
    },
    {
      deltaSpyS6: new BN(150_000),
      deltaQqqS6: new BN(100_000),
      deltaIwmS6: new BN(75_000),
      liveNoteCount: 1,
      merkleRoot: Array.from(Buffer.alloc(32, 7)),
      productProgramId: programs.flagshipAutocall.programId,
      publicationCid: fixedBytes("bafybeigdyrztintegrationdelta", 64),
      pythPublishTimes: [
        new BN(manifest.baseTimestamp),
        new BN(manifest.baseTimestamp),
        new BN(manifest.baseTimestamp),
      ],
      spotSpyS6: new BN(100_000_000),
      spotQqqS6: new BN(100_000_000),
      spotIwmS6: new BN(100_000_000),
    }
  );

  const solLookup = await createAndRegisterLookupTable(
    provider,
    programs.kernel,
    admin,
    pdas.protocolConfig,
    programs.solAutocall.programId,
    [
      programs.kernel.programId,
      pdas.protocolConfig,
      sol.productRegistryEntry,
      sol.authority,
    ]
  );
  sol.lookupTable = solLookup.lookupTable;
  sol.lookupTableRegistry = solLookup.lookupTableRegistry;

  const ilLookup = await createAndRegisterLookupTable(
    provider,
    programs.kernel,
    admin,
    pdas.protocolConfig,
    programs.ilProtection.programId,
    [
      programs.kernel.programId,
      pdas.protocolConfig,
      il.productRegistryEntry,
      il.authority,
    ]
  );
  il.lookupTable = ilLookup.lookupTable;
  il.lookupTableRegistry = ilLookup.lookupTableRegistry;

  const flagshipLookup = await createAndRegisterLookupTable(
    provider,
    programs.kernel,
    admin,
    pdas.protocolConfig,
    programs.flagshipAutocall.programId,
    [
      programs.kernel.programId,
      pdas.protocolConfig,
      flagship.productRegistryEntry,
      flagship.authority,
    ]
  );
  flagship.lookupTable = flagshipLookup.lookupTable;
  flagship.lookupTableRegistry = flagshipLookup.lookupTableRegistry;

  return {
    admin,
    adminUsdc,
    buyers: fundedBuyers,
    depositors: {
      juniorUsdc,
      senior: seniorDepositor,
      seniorUsdc,
    },
    keepers,
    manifest,
    oracles,
    pdas,
    products: {
      flagship,
      il,
      sol,
    },
    programs,
    provider,
    usdcMint,
  };
}
