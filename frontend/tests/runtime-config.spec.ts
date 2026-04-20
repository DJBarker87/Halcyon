import { test, expect } from "@playwright/test";

const TEST_ADDRESS = "11111111111111111111111111111111";

test("flagship shows missing runtime values by default", async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => window.localStorage.clear());
  await page.goto("/flagship");

  await expect(page.getByRole("heading", { level: 1, name: "Flagship Worst-of Equity Autocall" })).toBeVisible();
  await expect(page.getByText("Missing runtime values")).toBeVisible();
  await expect(page.getByText("Pyth SPY account")).toBeVisible();
  await expect(page.getByRole("button", { name: "Preview quote" })).toBeDisabled();
});

test("runtime config persists cluster-local account wiring", async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => window.localStorage.clear());
  await page.goto("/flagship");

  await page.getByRole("banner").getByRole("button", { name: "Runtime Config" }).click();
  await page.getByLabel("SPY price account").fill(TEST_ADDRESS);
  await page.getByLabel("QQQ price account").fill(TEST_ADDRESS);
  await page.getByLabel("IWM price account").fill(TEST_ADDRESS);
  await page.getByLabel("SOL price account").fill(TEST_ADDRESS);
  await page.getByLabel("USDC price account").fill(TEST_ADDRESS);
  await page.keyboard.press("Escape");

  await page.reload();
  await page.getByRole("banner").getByRole("button", { name: "Runtime Config" }).click();

  await expect(page.getByLabel("SPY price account")).toHaveValue(TEST_ADDRESS);
  await expect(page.getByLabel("QQQ price account")).toHaveValue(TEST_ADDRESS);
  await expect(page.getByLabel("IWM price account")).toHaveValue(TEST_ADDRESS);
});
