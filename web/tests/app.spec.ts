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
  await expect(page.locator("#preview tr.bad")).toHaveCount(1); // row 17 is in the preview, row 42 is not

  // The summary counts the suspicious rows, which are listed in full and can be exported.
  await expect(page.locator("#summary")).toContainText(/⚠ \d+ suspicious rows?/);
  const suspects = page.locator("#suspicious-table tbody tr");
  await expect(suspects.first()).toBeVisible();
  const row42 = page.locator('#suspicious-table tr[data-row="42"]');
  await expect(row42).toContainText("used 0.1 instead of 0.21");
  await expect(row42.locator("td.bad-cell")).toHaveText("29.97");
  // Row 17 has a wrong total, which also breaks net = total - vat: only the total is blamed.
  const row17 = page.locator('#suspicious-table tr[data-row="17"]');
  await expect(row17.locator("td.bad-cell")).toHaveText("94.3");
  await expect(row17).toContainText("the total in this row looks wrong");
  const [file] = await Promise.all([page.waitForEvent("download"), page.getByRole("button", { name: "Export as CSV" }).click()]);
  expect(file.suggestedFilename()).toBe("invoices.suspicious.csv");
  const csv = await (await import("node:fs/promises")).readFile(await file.path(), "utf8");
  expect(csv.split("\r\n")[0]).toBe("row,invoice,product,price,quantity,discount_pct,subtotal,net,vat,total,why");
  expect(csv).toContain("used 0.1 instead of 0.21");
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

test("a group of rows with another rule is not called a mistake", async ({ page }) => {
  // 37 rows with 21% VAT and 3 with 10%: too many to be typing mistakes.
  const lines = ["net,vat"];
  for (let i = 1; i <= 40; i++) {
    const net = 10 + i * 7;
    const rate = i % 13 === 0 ? 0.1 : 0.21;
    lines.push(`${net},${(net * rate).toFixed(2)}`);
  }
  await page.goto("./");
  await page.locator("#file").setInputFiles({ name: "rates.csv", mimeType: "text/csv", buffer: Buffer.from(lines.join("\n")) });
  await page.getByRole("button", { name: "Find derived columns" }).click();
  const vat = page.locator('article[data-column="vat"]');
  await expect(vat).toContainText("37 of 40 rows", { timeout: 45_000 });
  await expect(vat.locator(".note")).toContainText("8% of the rows follow another rule");
  await expect(page.locator("#summary")).toContainText("No suspicious rows");
  await expect(page.locator("#suspicious")).toBeHidden();
});

test("tests a formula written by the user, fitting its constants", async ({ page }) => {
  await page.goto("./");
  await page.getByRole("button", { name: /Try an example/ }).click();
  await page.getByText("Test your own formula").click();
  await page.getByLabel("Column to test").selectOption("vat");
  await page.getByLabel("Formula", { exact: true }).fill("vat = rate * net");
  await page.getByRole("button", { name: "Check" }).click();
  const card = page.locator("#own-result article");
  await expect(card).toContainText("vat = 0.21 * net");
  await expect(card).toContainText("59 of 60 rows");

  await page.getByLabel("Formula", { exact: true }).fill("price * (");
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.locator("#own-error")).toBeVisible();
});

test("search settings are applied and remembered", async ({ page }) => {
  await page.goto("./");
  await page.getByRole("button", { name: /Try an example/ }).click();
  await page.getByText("Search settings").click();
  await page.getByLabel(/Fast/).check();
  await page.locator("#only").selectOption("vat");
  await expect(page.locator("#settings-summary")).toHaveText("(Fast)");
  await page.getByLabel("÷ divide").uncheck();
  await expect(page.locator("#settings-summary")).toHaveText("(Fast, 9 of 10 operations)");

  await page.getByRole("button", { name: "Find derived columns" }).click();
  await expect(page.locator('article[data-column="vat"]')).toContainText("vat = 0.21 * net", { timeout: 45_000 });
  await expect(page.locator("#cards article")).toHaveCount(1);

  await page.reload();
  await page.getByRole("button", { name: /Try an example/ }).click();
  await page.getByText("Search settings").click();
  await expect(page.getByLabel(/Fast/)).toBeChecked();
  await expect(page.getByLabel("÷ divide")).not.toBeChecked();
  await page.getByRole("button", { name: "Reset to the defaults" }).click();
  await expect(page.locator("#settings-summary")).toHaveText("(Normal)");
});
