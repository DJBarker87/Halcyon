import { test, expect } from "@playwright/test";

/**
 * Audit F3 — runtime config hardening.
 *
 * The pre-audit frontend read arbitrary fields from localStorage and
 * merged them into runtime state; a poisoned browser could rewire RPC
 * endpoint, program IDs, and oracle accounts without the user knowing.
 *
 * These tests exercise the post-audit behaviour:
 *
 *   1. Only the cluster id is read from localStorage. Unknown ids fall
 *      back to the default cluster. Arbitrary fields are ignored.
 *   2. Cluster changes require explicit confirmation via a modal.
 *   3. A cluster/genesis-hash mismatch blocks the app before the wallet
 *      provider mounts, and leaves recovery paths in the runtime panel.
 */

const RUNTIME_CONFIG_STORAGE_KEY_V2 = "halcyon-layer5-runtime-config-v2";
const SOLANA_DEVNET_GENESIS = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
const SOLANA_MAINNET_GENESIS = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";

// Stub genesis-hash responses so tests don't hit live Solana RPCs.
async function stubGenesisHash(page: import("@playwright/test").Page, hash: string) {
  await page.route("**/*", async (route) => {
    const request = route.request();
    if (request.method() === "POST") {
      const body = request.postData() ?? "";
      if (body.includes("getGenesisHash")) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ jsonrpc: "2.0", id: 1, result: hash }),
        });
        return;
      }
    }
    await route.continue();
  });
}

test("unknown cluster id in localStorage falls back to the default cluster", async ({ page }) => {
  await page.goto("/");
  await page.evaluate((key) => {
    window.localStorage.setItem(key, JSON.stringify({ cluster: "pluto" }));
  }, RUNTIME_CONFIG_STORAGE_KEY_V2);
  await page.reload();

  // Open settings panel and confirm we landed on an allowlisted cluster.
  await page.getByRole("banner").getByRole("button", { name: "Network settings" }).click();
  // The cluster radio for the fallback cluster is aria-checked=true. We
  // accept any of the three because the exact default depends on NODE_ENV
  // in the test build.
  const selected = page.getByRole("radio", { checked: true });
  await expect(selected).toHaveCount(1);
});

test("arbitrary localStorage fields are ignored; only cluster id is honoured", async ({ page }) => {
  await page.goto("/");
  await page.evaluate((key) => {
    window.localStorage.setItem(
      key,
      JSON.stringify({
        cluster: "localnet",
        // Pre-audit payload would include these; they must be discarded.
        settings: {
          devnet: {
            rpcUrl: "https://evil.example.com",
            flagshipProgramId: "11111111111111111111111111111111",
            pythSpy: "11111111111111111111111111111111",
          },
        },
        rpcUrl: "https://evil.example.com",
      }),
    );
  }, RUNTIME_CONFIG_STORAGE_KEY_V2);
  await page.reload();

  const runtimeConfigButton = page.getByRole("banner").getByRole("button", { name: "Network settings" });
  await expect(runtimeConfigButton).toBeVisible();
  await runtimeConfigButton.click();
  // The pinned-wiring section renders the live config; no evil RPC here.
  const pinnedRpc = page.getByText("https://evil.example.com");
  await expect(pinnedRpc).toHaveCount(0);
});

test("changing cluster requires explicit confirmation via modal", async ({ page }) => {
  await stubGenesisHash(page, SOLANA_MAINNET_GENESIS);
  await page.goto("/");
  await page.evaluate((key) => window.localStorage.setItem(key, JSON.stringify({ cluster: "localnet" })), RUNTIME_CONFIG_STORAGE_KEY_V2);
  await page.reload();

  await page.getByRole("banner").getByRole("button", { name: "Network settings" }).click();
  await page.getByRole("radio", { name: /Mainnet/ }).click();

  // Modal shown; cluster not yet switched.
  const modal = page.getByRole("dialog", { name: "Confirm cluster change" });
  await expect(modal).toBeVisible();

  // Cancel — nothing changes.
  await modal.getByRole("button", { name: "Cancel" }).click();
  await expect(modal).toHaveCount(0);
  await expect(page.getByRole("radio", { name: /Localnet/, checked: true })).toBeVisible();

  // Re-open, confirm the switch.
  await page.getByRole("radio", { name: /Mainnet/ }).click();
  const modal2 = page.getByRole("dialog", { name: "Confirm cluster change" });
  await expect(modal2).toBeVisible();
  await modal2.getByRole("button", { name: /Switch to Mainnet/ }).click();
  await expect(modal2).toHaveCount(0);
  const runtimeConfigButton = page.getByRole("banner").getByRole("button", { name: "Network settings" });
  await expect(runtimeConfigButton).toBeVisible();
  await expect(page.getByRole("banner")).toContainText("mainnet");
  await runtimeConfigButton.click();
  await expect(page.getByRole("radio", { name: /Mainnet/, checked: true })).toBeVisible();
});

test("genesis-hash mismatch blocks the app before wallet providers mount", async ({ page }) => {
  await stubGenesisHash(page, "NotTheRealGenesisHashHere1111111111111111111");
  await page.goto("/");
  await page.getByRole("banner").getByRole("button", { name: "Network settings" }).click();
  await page.getByRole("radio", { name: /Devnet/ }).click();
  const modal = page.getByRole("dialog", { name: "Confirm cluster change" });
  await expect(modal).toBeVisible();
  await modal.getByRole("button", { name: /Switch to Devnet/ }).click();
  await expect(modal).toHaveCount(0);

  await expect(page.getByTestId("genesis-check-blocked")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("genesis-check-blocked")).toContainText("Cluster verification failed");
  await expect(page.getByRole("button", { name: /connect wallet|select wallet/i })).toHaveCount(0);
  await page.getByRole("button", { name: "Runtime Config" }).click();
  await expect(page.getByRole("dialog", { name: "Runtime configuration" })).toBeVisible();
});
