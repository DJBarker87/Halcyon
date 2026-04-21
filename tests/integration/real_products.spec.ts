import { BN } from "@coral-xyz/anchor";
import { PublicKey, SYSVAR_CLOCK_PUBKEY } from "@solana/web3.js";
import { assert, expect } from "chai";

import { setupFullProtocol, TestContext } from "./setup";

describe("real product integration", function () {
  this.timeout(1_000_000);

  let ctx: TestContext;

  before(async () => {
    ctx = await setupFullProtocol();
  });

  it("boots the full protocol and previews all three real products", async () => {
    const protocolConfigInfo = await ctx.provider.connection.getAccountInfo(
      ctx.pdas.protocolConfig,
      "confirmed"
    );
    assert(protocolConfigInfo !== null);
    assert(protocolConfigInfo.owner.equals(ctx.programs.kernel.programId));

    const solPreview = (await ctx.programs.solAutocall.methods
      .previewQuote(new BN(5_000_000_000))
      .accounts({
        clock: SYSVAR_CLOCK_PUBKEY,
        productRegistryEntry: ctx.products.sol.productRegistryEntry,
        protocolConfig: ctx.pdas.protocolConfig,
        pythSol: new PublicKey(ctx.oracles["sol-entry"].pubkey),
        regimeSignal: ctx.products.sol.regimeSignal,
        vaultSigma: ctx.products.sol.vaultSigma,
      } as any)
      .view()) as any;
    expect(new BN(solPreview.maxLiability).gt(new BN(0))).to.eq(true);
    expect(new BN(solPreview.entryPriceS6).gt(new BN(0))).to.eq(true);

    const ilPreview = (await ctx.programs.ilProtection.methods
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
      .view()) as any;
    expect(new BN(ilPreview.premium).gt(new BN(0))).to.eq(true);
    expect(new BN(ilPreview.maxLiability).gt(new BN(0))).to.eq(true);

    const flagshipPreview = (await ctx.programs.flagshipAutocall.methods
      .previewQuote(new BN(5_000_000_000))
      .accounts({
        clock: SYSVAR_CLOCK_PUBKEY,
        productRegistryEntry: ctx.products.flagship.productRegistryEntry,
        protocolConfig: ctx.pdas.protocolConfig,
        pythIwm: new PublicKey(ctx.oracles["flagship-iwm-entry"].pubkey),
        pythQqq: new PublicKey(ctx.oracles["flagship-qqq-entry"].pubkey),
        pythSpy: new PublicKey(ctx.oracles["flagship-spy-entry"].pubkey),
        regression: ctx.products.flagship.regression,
        vaultSigma: ctx.products.flagship.vaultSigma,
      } as any)
      .view()) as any;
    expect(new BN(flagshipPreview.maxLiability).gt(new BN(0))).to.eq(true);
    expect(new BN(flagshipPreview.entrySpyPriceS6).gt(new BN(0))).to.eq(true);
  });
});
