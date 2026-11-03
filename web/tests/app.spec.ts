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
  await expect(vat.locator("tbody tr")).toHaveText(["4229.9762.937used 0.1 instead of 0.21"]);

  await expect(page.locator('article[data-column="subtotal"]')).toContainText("✓ all 60 rows");

  // VAT is a known formula, with the tax rate read from the data.
  await vat.getByText("Known formula").click();
  await expect(vat.locator(".known-card")).toContainText("Tax amount");
  await expect(vat.locator(".known-card")).toContainText("0.21");
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

test("searches the formula library", async ({ page }) => {
  await page.goto("./library.html");
  await expect(page.getByRole("heading", { name: "Library of known formulas" })).toBeVisible();
  await expect(page.locator("#count")).toContainText(/of \d{3} formulas/);
  await page.getByLabel("Search").fill("kinetic");
  await expect(page.locator("article:not([hidden]) h2").first()).toContainText(/kinetic/i);
  await page.getByLabel("Search").fill("");
  await page.getByLabel("Domain").selectOption("finance");
  await expect(page.locator("#line_total")).toBeVisible();
  await expect(page.locator("#kinetic_energy")).toBeHidden();
});

test("opens the library on a linked formula", async ({ page }) => {
  await page.goto("./library.html#kinetic_energy");
  await expect(page.locator("#kinetic_energy")).toHaveClass(/target/);
});
