import { test, expect } from "@playwright/test";

test("localnet burner wallet can connect and disconnect", async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => window.localStorage.clear());
  await page.goto("/flagship");

  const walletTrigger = page.getByRole("button", { name: /connect wallet|select wallet/i });
  await walletTrigger.click();
  await page.getByRole("button", { name: /burner wallet/i }).click();

  const connectedButton = page.locator(".wallet-adapter-button").first();
  await expect(connectedButton).not.toContainText(/connect wallet|select wallet/i);

  await connectedButton.click();
  await page.getByText("Disconnect").click();
  await expect(page.getByRole("button", { name: /connect wallet|select wallet/i })).toBeVisible();
});
