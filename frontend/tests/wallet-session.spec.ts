import { test, expect } from "@playwright/test";

const RUNTIME_CONFIG_STORAGE_KEY_V2 = "halcyon-layer5-runtime-config-v2";

test("localnet burner wallet can connect and disconnect", async ({ page }) => {
  await page.goto("/");
  await page.evaluate((key) => {
    window.localStorage.setItem(key, JSON.stringify({ cluster: "localnet" }));
  }, RUNTIME_CONFIG_STORAGE_KEY_V2);
  await page.goto("/flagship");
  await expect(page.getByRole("banner")).toContainText("localnet");

  const walletTrigger = page.getByRole("button", { name: /connect wallet|select wallet/i });
  await walletTrigger.click();
  await page.getByRole("button", { name: /burner wallet/i }).click();

  const walletControl = page.getByTestId("wallet-control-button");
  await expect(walletControl).toBeVisible();
  const currentLabel = (await walletControl.textContent()) ?? "";
  if (/connect wallet|select wallet/i.test(currentLabel)) {
    await walletControl.click();
    await expect(walletControl).not.toContainText(/connect wallet|select wallet|connecting/i);
  }

  await walletControl.click();
  const menu = page.getByTestId("wallet-control-menu");
  await expect(menu).toBeVisible();
  await menu.getByRole("menuitem", { name: "Disconnect" }).click();
  await expect(page.getByRole("button", { name: /connect wallet|select wallet/i })).toBeVisible();
});
