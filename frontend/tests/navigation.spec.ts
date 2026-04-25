import { test, expect } from "@playwright/test";

const ROUTES = [
  { path: "/demo", heading: "Live devnet note receipt" },
  { path: "/flagship", heading: "SPY · QQQ · IWM coupon note" },
  { path: "/sol-autocall", heading: "Principal-backed SOL note" },
  { path: "/il-protection", heading: "Two ways to cover impermanent loss on SOL/USDC." },
  { path: "/stress-tests", heading: "Backtest Explorer" },
  { path: "/portfolio", heading: "Your notes" },
  { path: "/lending-demo", heading: "Receipt-token collateral desk" },
  { path: "/faucet", heading: "mockUSDC for judge wallets" },
  { path: "/vault", heading: "Shared kernel capital state" },
];

for (const route of ROUTES) {
  test(`renders ${route.path}`, async ({ page }) => {
    await page.goto("/");
    await page.evaluate(() => window.localStorage.clear());
    await page.goto(route.path);
    await expect(page.getByRole("heading", { level: 1, name: route.heading })).toBeVisible();
    await expect(
      page.getByRole("banner").getByRole("button", { name: "Network settings" }),
    ).toBeVisible();
  });
}
