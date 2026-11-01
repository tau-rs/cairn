import { test, expect } from "@playwright/test";

// Open the graph view and its settings panel (the gear reveals the Local /
// Groups / Forces panels). The graph itself renders to a <canvas>, so these
// tests assert the DOM-observable chrome + localStorage persistence, not the
// canvas node set (the cap/filter effects on the node set are unit-tested in
// globalCap / graphFilter / GraphView component tests).
async function openGraphPanel(page: import("@playwright/test").Page) {
  const sidebar = page.locator("aside").first();
  await expect(
    sidebar.getByRole("button", { name: "ideas", exact: true }),
  ).toBeVisible(); // app loaded (mock fixture)
  // The view persists across reload, so only flip to graph if we aren't there
  // already (in graph view the toggle reads "Editor", not "Graph").
  const graphBtn = page.getByRole("button", { name: /^graph$/i });
  if (await graphBtn.count()) await graphBtn.click();
  await expect(page.locator("canvas").first()).toBeVisible();
  await page.getByRole("button", { name: "Graph forces" }).click();
}

test("graph filters: min-degree, ungrouped and recency controls render", async ({
  page,
}) => {
  await page.goto("/");
  await openGraphPanel(page);
  await expect(page.getByLabel("Minimum degree")).toBeVisible();
  await expect(
    page.getByRole("button", { name: /toggle ungrouped notes/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("checkbox", { name: /recent edits/i }),
  ).toBeVisible();
  // The window slider is hidden until recency is enabled.
  await expect(page.getByLabel(/recency window/i)).toHaveCount(0);
});

test("graph recency: enabling reveals the window slider and persists on reload", async ({
  page,
}) => {
  await page.goto("/");
  await openGraphPanel(page);
  await page.getByRole("checkbox", { name: /recent edits/i }).check();
  await expect(page.getByLabel(/recency window/i)).toBeVisible();

  // Reload → reopen the panel → the recency toggle is still on (localStorage).
  await page.reload();
  await openGraphPanel(page);
  await expect(
    page.getByRole("checkbox", { name: /recent edits/i }),
  ).toBeChecked();
});

test("graph tag-group filter: hiding a group persists across reload", async ({
  page,
}) => {
  await page.goto("/");
  await openGraphPanel(page);

  // Add a tag group for the fixture tag "rust", then hide it via its eye toggle.
  await page.getByRole("button", { name: /add group/i }).click();
  await page.getByLabel("Group kind").last().selectOption("tag");
  await page.getByLabel("Group query").last().fill("rust");
  const eye = page.getByRole("button", { name: /toggle group rust/i });
  await eye.click();
  await expect(eye).toHaveAttribute("aria-pressed", "true");

  // Reload → the group and its hidden state persist (colorGroups + filter).
  await page.reload();
  await openGraphPanel(page);
  await expect(
    page.getByRole("button", { name: /toggle group rust/i }),
  ).toHaveAttribute("aria-pressed", "true");
});
