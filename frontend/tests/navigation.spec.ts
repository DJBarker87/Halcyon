import { test, expect } from "@playwright/test";

const ROUTES = [
  { path: "/flagship", heading: "Flagship Worst-of Equity Autocall" },
  { path: "/sol-autocall", heading: "SOL Autocall" },
  { path: "/il-protection", heading: "IL Protection" },
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
      page.getByRole("banner").getByRole("button", { name: "Runtime Config" }),
    ).toBeVisible();
  });
}
