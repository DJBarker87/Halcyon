import { test, expect } from "@playwright/test";

test("localnet burner wallet can connect and disconnect", async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => window.localStorage.clear());
  await page.goto("/flagship");

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
