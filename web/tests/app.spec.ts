import { expect, test } from "@playwright/test";

test("finds derived columns in the example invoices", async ({ page }) => {
  await page.goto("./");
  await expect(page.getByRole("heading", { name: "Find the formulas behind your data" })).toBeVisible();

  await page.getByRole("button", { name: /Try an example/ }).click();
  await expect(page.locator("#file-summary")).toHaveText("60 rows, 8 numeric columns, 1 text");
  // The invoice number is recognised as an ID and left out.
  await expect(page.locator('input[data-column="invoice"]')).not.toBeChecked();

  await page.getByRole("button", { name: "Find derived columns" }).click();
  const vat = page.locator('article[data-column="vat"]');
  await expect(vat).toContainText("vat = 0.21 * net", { timeout: 45_000 });
  await expect(vat).toContainText("59 of 60 rows");
  await vat.getByText("1 row breaks the rule").click();
  await expect(vat.locator("tbody tr")).toHaveText(["4229.9762.937"]);

  await expect(page.locator('article[data-column="subtotal"]')).toContainText("✓ all 60 rows");
  await expect(page.locator("tr.bad")).toHaveCount(1); // row 17 is in the preview, row 42 is not
});

test("uploads a file and reports unreadable ones", async ({ page }) => {
  await page.goto("./");
  await page.locator("#file").setInputFiles({
    name: "broken.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("name,city\nAnna,Girona\n"),
  });
  await expect(page.getByRole("alert")).toContainText("No numeric columns");
});
