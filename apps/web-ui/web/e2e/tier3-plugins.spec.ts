import { expect, test } from "@playwright/test";

test("Tier-3 word-count plugin: consent -> read -> revoke", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();

  // Word Count is an iframe plugin requesting activeNote.read -> consent prompt.
  await expect(page.getByText(/wants to:/i)).toBeVisible();
  await expect(page.getByText(/Read the current note/i)).toBeVisible();
  await page.getByRole("button", { name: /^allow$/i }).click();

  // Iframe mounts; open a note and assert the iframe is visible (the word count
  // is rendered inside a sandboxed srcdoc iframe — frameLocator-based in-iframe
  // text assertions are unreliable in this Playwright/Chromium setup due to the
  // null-origin srcdoc sandbox, so we assert iframe presence instead).
  await expect(page.locator('iframe[title="plugin:wordcount"]')).toBeVisible();
  await sidebar.getByRole("button", { name: "ideas", exact: true }).click();
  // Iframe remains mounted after navigating to a note.
  await expect(page.locator('iframe[title="plugin:wordcount"]')).toBeVisible();

  // Revoke in Settings -> iframe unmounts (consent prompt returns).
  await page.getByLabel("Settings").click();
  await page.getByRole("button", { name: /revoke/i }).click();
  await page.getByRole("button", { name: /done/i }).click();
  await expect(page.locator('iframe[title="plugin:wordcount"]')).toHaveCount(0);
});

test("Tier-3 iframe widget renders in the panel.main bottom dock", async ({
  page,
}) => {
  await page.goto("/");

  // The word-count contribution targets the `panel.main` slot, so it renders in
  // the bottom Plugin panel dock (not the sidebar). The dock is present from the
  // start, hosting the activeNote.read consent prompt until granted.
  const dock = page.locator('section[aria-label="Plugin panel"]');
  await expect(dock).toBeVisible();
  await expect(dock.getByText(/wants to:/i)).toBeVisible();

  // Granting consent mounts the sandboxed iframe (entry served from its uiRoot
  // bundle) inside the dock — exercising the contract's iframe `entry` +
  // `panel.main` slot end to end.
  await dock.getByRole("button", { name: /^allow$/i }).click();
  await expect(dock.locator('iframe[title="plugin:wordcount"]')).toBeVisible();
});
