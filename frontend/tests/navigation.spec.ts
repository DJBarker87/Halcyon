import { test, expect } from "@playwright/test";

const ROUTES = [
  { path: "/flagship", heading: "SPY · QQQ · IWM coupon note" },
  { path: "/sol-autocall", heading: "Principal-backed SOL note" },
  { path: "/il-protection", heading: "Impermanent-loss cover" },
  { path: "/portfolio", heading: "Wallet policies across every product" },
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
