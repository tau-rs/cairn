import { test, expect } from "@playwright/test";

test("create, edit, autosave, search, backlink, versions", async ({ page }) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();

  // Fixture notes are listed.
  await expect(
    sidebar.getByRole("button", { name: "index", exact: true }),
  ).toBeVisible();
  await expect(
    sidebar.getByRole("button", { name: "ideas", exact: true }),
  ).toBeVisible();

  // Open a note; its backlinks show (index.md links to ideas).
  await sidebar.getByRole("button", { name: "ideas", exact: true }).click();
  // Anchor on the right-aside "Backlinks" tab: the panel-heading span and the
  // "No backlinks" empty-state also read "Backlinks", so a plain text match
  // strict-mode-violates. The tab is the unambiguous, always-present element;
  // the backlink content itself is asserted on the next line.
  await expect(page.getByRole("tab", { name: "Backlinks" })).toBeVisible();
  await expect(
    page.locator("aside").last().getByRole("button", { name: "index.md" }),
  ).toBeVisible();

  // Create a new note that links to ideas (via the styled modal).
  await page.getByRole("button", { name: /new note/i }).click();
  const newNoteDialog = page.getByRole("dialog");
  await newNoteDialog.getByPlaceholder("notes/idea.md").fill("fresh.md");
  await newNoteDialog.getByRole("button", { name: /^create$/i }).click();
  // New note opens editable in live preview — type directly into CodeMirror.
  const cm = page.locator(".cm-content");
  await cm.click();
  await page.keyboard.type("a new note pointing at [[ideas]]");
  // Move cursor to start of line so the cursor no longer touches the wikilink;
  // live preview then renders the [[ideas]] as a widget.
  await page.keyboard.press("Home");
  // In live preview the wikilink renders as a clickable widget (not raw [[…]]).
  await expect(
    page.locator(".cm-lp-wikilink", { hasText: "ideas" }).first(),
  ).toBeVisible();

  // Autosave fires after the debounce; status returns to Saved.
  await expect(page.getByText(/saved/i)).toBeVisible({ timeout: 5000 });

  // Search finds the new note by body text.
  await page.getByPlaceholder("Search…").fill("pointing");
  await page.getByPlaceholder("Search…").press("Enter");
  const results = page.getByTestId("search-results");
  await expect(results.getByText(/Results/)).toBeVisible();
  // Scope to the overlay: "fresh.md" also exists in the note list.
  await results.getByRole("button", { name: "fresh.md" }).click();

  // ideas.md now has fresh.md as a backlink.
  await sidebar.getByRole("button", { name: "ideas", exact: true }).click();
  await expect(
    page.locator("aside").last().getByRole("button", { name: "fresh.md" }),
  ).toBeVisible();

  // The StatusBar shows the save + sync axes; its 🕘 Versions button opens
  // the Versions browser in the right aside.
  const statusBar = page.getByTestId("status-bar");
  await expect(statusBar).toContainText(/saved/i);
  await expect(statusBar).toContainText("Synced");
  await statusBar.getByRole("button", { name: /versions/i }).click();
  await expect(page.getByRole("tab", { name: "Versions" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  // ideas.md was never edited in this test, so its version list is empty.
  await expect(page.getByText("No versions yet.")).toBeVisible();
});

test("versions: a note switch seals a generated version that can be named", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();

  // Edit ideas.md, then switch notes — the seal hint fires on the switch and
  // flushes the pending autosave, so the version carries the last keystrokes.
  await sidebar.getByRole("button", { name: "ideas", exact: true }).click();
  await page.locator(".cm-content").click();
  await page.keyboard.type(" plus four more words");
  await sidebar.getByRole("button", { name: "todo", exact: true }).click();

  // Back on ideas.md, the Versions browser shows the engine-style generated
  // message with its word delta.
  await sidebar.getByRole("button", { name: "ideas", exact: true }).click();
  await page
    .getByTestId("status-bar")
    .getByRole("button", { name: /versions/i })
    .click();
  const versions = page.locator("aside").last();
  // Titled by display_title (`# Ideas`), matching the engine's template.
  const row = versions.getByText(/^Edit "Ideas" \(\+\d+ words\)$/);
  await expect(row).toBeVisible();
  await expect(versions.getByText(/\+\d+\/−\d+ words/)).toBeVisible();

  // Name… tags it; the badge appears and the "Named only" filter finds it.
  await versions.getByRole("button", { name: /name…/i }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByPlaceholder("e.g. Draft 1").fill("Draft 1");
  await dialog.getByRole("button", { name: /name version/i }).click();
  await expect(versions.getByText("Draft 1")).toBeVisible();
  await versions.getByLabel(/named only/i).check();
  await expect(versions.getByText("Draft 1")).toBeVisible();
});

test("graph view: toggle shows the force-graph canvas", async ({ page }) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await expect(
    sidebar.getByRole("button", { name: "ideas", exact: true }),
  ).toBeVisible(); // app loaded (mock fixture)

  await page.getByRole("button", { name: /^graph$/i }).click();

  // The force-graph renders a <canvas>; the toggle flips to "Editor".
  await expect(page.locator("canvas").first()).toBeVisible();
  await expect(page.getByRole("button", { name: /^editor$/i })).toBeVisible();

  // The forces gear opens the settings panel.
  await page.getByRole("button", { name: "Graph forces" }).click();
  await expect(page.getByLabel("Center force")).toBeVisible();

  // Color groups: the Groups section is present and Add group adds a row.
  await expect(page.getByText("Groups", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /add group/i }).click();
  await expect(page.getByLabel("Group query")).toBeVisible();
});

test("graph local mode: open a note, switch to Local, canvas renders", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  // Open a note so the graph has a root.
  await sidebar.getByRole("button", { name: "index", exact: true }).click();
  // Switch to the graph view.
  await page.getByRole("button", { name: /^graph$/i }).click();
  // Toggle to Local — the canvas (now the index.md neighborhood) still renders.
  await page.getByRole("button", { name: "Local" }).click();
  await expect(page.locator("canvas").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Local" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("live preview: heading styled, wikilink opens note, source toggle shows raw", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await sidebar.getByRole("button", { name: "index", exact: true }).click();

  // Heading is styled in live preview (the "# " marker is hidden).
  await expect(page.locator(".cm-lp-h1")).toBeVisible();

  // [[ideas]] is a clickable widget; clicking opens ideas.md.
  await page.locator(".cm-lp-wikilink", { hasText: "ideas" }).first().click();
  await expect(page.locator(".cm-content")).toContainText("Ideas");

  // Toggle to Source → raw markdown ("# Ideas") visible.
  await page.getByRole("button", { name: /^source$/i }).click();
  await expect(page.locator(".cm-content")).toContainText("# Ideas");
});

test("document live-preview: blocks render, checkbox toggles, code reveals raw", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await sidebar
    .getByRole("button", { name: "kitchensink", exact: true })
    .click();

  // Block elements render in live preview (load-bearing: the StateField-based
  // block table widget must render in a real browser without throwing).
  await expect(page.locator(".cm-lp-table")).toBeVisible();
  await expect(page.locator("img.cm-lp-img")).toBeVisible();
  await expect(page.locator(".cm-lp-hr")).toBeVisible();
  await expect(page.locator(".cm-lp-codeblock").first()).toBeVisible();
  await expect(page.locator(".cm-lp-bullet").first()).toBeVisible();
  // The rendered table is a real <table> with the fixture's cells.
  await expect(
    page.locator(".cm-lp-table").getByRole("columnheader", { name: "A" }),
  ).toBeVisible();
  await expect(
    page.locator(".cm-lp-table").getByRole("cell", { name: "1" }),
  ).toBeVisible();

  // The open task renders an unchecked checkbox; clicking it checks the source.
  const openTask = page.locator(".cm-lp-task.unchecked").first();
  await expect(openTask).toBeVisible();
  await openTask.click();
  // After toggle the source has one more checked task than before (now two).
  await expect(page.locator(".cm-lp-task.checked")).toHaveCount(2);

  // Reveal-on-cursor: clicking into the fenced code block places the caret on
  // its line and swaps the rendered block back to its raw markdown source.
  await page.locator(".cm-lp-codeblock").first().click();
  await expect(page.getByText("```js").first()).toBeVisible();
  await expect(page.getByText("const x = 1;").first()).toBeVisible();
});

test("click-to-edit: blockquote, code block, and image reveal raw on click", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await sidebar
    .getByRole("button", { name: "kitchensink", exact: true })
    .click();
  const content = page.locator(".cm-content");

  // Blockquote: clicking its text reveals the raw "> " marker.
  await page.getByText("a quoted line").click();
  await expect(content).toContainText("> a quoted line");

  // Code block: clicking inside reveals the ``` fences.
  await page.getByText("const x = 1;").click({ position: { x: 4, y: 8 } });
  await expect(content).toContainText("```");

  // Image: clicking the rendered <img> reveals its raw markdown and removes the img.
  await expect(page.locator("img.cm-lp-img")).toBeVisible();
  await page.locator("img.cm-lp-img").click();
  await expect(content).toContainText("![logo](img/logo.png)");
  await expect(page.locator("img.cm-lp-img")).toHaveCount(0);
});

test("table editor: click to edit a cell and commit on click-away", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await sidebar
    .getByRole("button", { name: "kitchensink", exact: true })
    .click();
  const content = page.locator(".cm-content");

  // Click the rendered table → it becomes editable.
  await page.locator(".cm-lp-table").first().click();
  const firstCell = page
    .locator(".cm-lp-table.editing th, .cm-lp-table.editing td")
    .first();
  await expect(firstCell).toBeVisible();

  // Edit a body cell, then click away to commit.
  const cell = page.locator(".cm-lp-table.editing td").first();
  await cell.click();
  await expect(cell).toBeFocused();
  // Select the cell's text via the Selection API rather than Ctrl+A. The cell
  // is a contentEditable inside CodeMirror, whose keymap binds Ctrl+A to a
  // whole-document selectAll; on a slow CI runner that wins the race, so
  // typing "X1" replaces the ENTIRE document (wiping the "Kitchen sink"
  // heading, after which the click-away locator times out). selectText()
  // scopes the selection to the cell, so "X1" only replaces the cell.
  await cell.selectText();
  await page.keyboard.type("X1");
  await page.getByText("Kitchen sink").click(); // click away
  // The committed value appears in the document source / re-rendered table.
  await expect(content).toContainText("X1");
});

test("table editor: add a row and a column, commit on click-away", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await sidebar
    .getByRole("button", { name: "kitchensink", exact: true })
    .click();

  await page.locator(".cm-lp-table").first().click();
  const rowsBefore = await page
    .locator(".cm-lp-table.editing tbody tr")
    .count();

  await page.locator(".cm-lp-add-row").click();
  await expect(page.locator(".cm-lp-table.editing tbody tr")).toHaveCount(
    rowsBefore + 1,
  );

  const colsBefore = await page
    .locator(".cm-lp-table.editing thead th")
    .count();
  await page.locator(".cm-lp-add-col").click();
  await expect(page.locator(".cm-lp-table.editing thead th")).toHaveCount(
    colsBefore + 1,
  );

  // Click away → commit; re-render read-only, then re-open to confirm persisted.
  await page.getByText("Kitchen sink").click();
  await page.locator(".cm-lp-table").first().click();
  await expect(page.locator(".cm-lp-table.editing tbody tr")).toHaveCount(
    rowsBefore + 1,
  );
  await expect(page.locator(".cm-lp-table.editing thead th")).toHaveCount(
    colsBefore + 1,
  );
});

test("table editor: Tab moves between cells", async ({ page }) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await sidebar
    .getByRole("button", { name: "kitchensink", exact: true })
    .click();
  await page.locator(".cm-lp-table").first().click();
  const cells = page.locator(
    ".cm-lp-table.editing th, .cm-lp-table.editing td",
  );
  await cells.first().click();
  await page.keyboard.press("Tab");
  // focus advanced to the second cell
  await expect(cells.nth(1)).toBeFocused();
});

test("command palette: ⌘K quick-opens a note and runs a command", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await expect(
    sidebar.getByRole("button", { name: "ideas", exact: true }),
  ).toBeVisible(); // app loaded

  // Open the palette (Control+k works on CI; the app listener accepts meta or ctrl).
  await page.keyboard.press("Control+k");
  const input = page.getByPlaceholder(/type a command/i);
  await expect(input).toBeVisible();

  // Quick-open a note.
  await input.fill("ideas");
  await page.keyboard.press("Enter");
  await expect(page.locator(".cm-content")).toContainText("Ideas"); // ideas.md opened

  // Re-open, run "Show versions" → the Versions pane opens in the right aside.
  await page.keyboard.press("Control+k");
  await page.getByPlaceholder(/type a command/i).fill("versions");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("tab", { name: "Versions" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  // ideas.md was only quick-opened (not edited), so its version list is empty.
  await expect(page.getByText("No versions yet.")).toBeVisible();
});

test("editor tabs: preview replaces, edit pins, close focuses, reload restores", async ({
  page,
}) => {
  await page.goto("/");
  // Note-list lives in the first aside; scope source clicks there so they
  // don't collide with backlink/tab buttons of the same name.
  const noteList = page.locator("aside").first();
  await expect(noteList.getByText("index", { exact: true })).toBeVisible(); // app loaded

  // Open index → a single preview tab.
  await noteList.getByRole("button", { name: "index", exact: true }).click();
  await expect(page.getByRole("tab", { name: /index/ })).toBeVisible();

  // Open ideas → the preview tab is REPLACED (index tab gone).
  await noteList.getByRole("button", { name: "ideas", exact: true }).click();
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /index/ })).toHaveCount(0);

  // Edit ideas → its tab pins. Open todo → a new preview tab (ideas stays).
  const cm = page.locator(".cm-content");
  await cm.click();
  await page.keyboard.type(" edited");
  await expect(page.getByText(/saved/i)).toBeVisible({ timeout: 5000 });
  await noteList.getByRole("button", { name: "todo", exact: true }).click();
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /todo/ })).toBeVisible();

  // Close the todo tab via its × → ideas remains.
  await page.getByRole("button", { name: "close todo" }).click();
  await expect(page.getByRole("tab", { name: /todo/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();

  // Reload → the pinned ideas tab is restored; the (preview) todo is not.
  await page.reload();
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /todo/ })).toHaveCount(0);
});

test("folder tree: nested note shows under its folder; collapse hides it; folder + pre-fills", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await expect(
    sidebar.getByRole("button", { name: "projects", exact: true }),
  ).toBeVisible();
  await expect(
    sidebar.getByRole("button", { name: "demo", exact: true }),
  ).toBeVisible();

  // Collapsing the folder hides its child.
  await sidebar.getByRole("button", { name: "projects", exact: true }).click();
  await expect(
    sidebar.getByRole("button", { name: "demo", exact: true }),
  ).toHaveCount(0);

  // Re-expand, then the folder + opens the dialog pre-filled with "projects/".
  await sidebar.getByRole("button", { name: "projects", exact: true }).click();
  await sidebar.getByRole("button", { name: "projects", exact: true }).hover();
  await sidebar.getByRole("button", { name: "new note in projects" }).click();
  await expect(page.getByPlaceholder("notes/idea.md")).toHaveValue("projects/");
});

test("keyboard shortcuts: rebind Open Settings, use it, and persist", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await expect(sidebar.getByText("index", { exact: true })).toBeVisible();

  // Open Settings via the gear, then rebind "Open Settings" to a free chord.
  await page.getByLabel("Settings").click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Keyboard shortcuts")).toBeVisible();
  await dialog.getByRole("button", { name: "rebind Open Settings" }).click();
  await dialog
    .getByLabel("press keys for Open Settings")
    .press("Control+Shift+S"); // Mod+Shift+S — unused by defaults
  // Close the dialog.
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);

  // The new chord reopens Settings.
  await page.keyboard.press("Control+Shift+S");
  await expect(
    page.getByRole("dialog").getByText("Keyboard shortcuts"),
  ).toBeVisible();

  // Persisted across reload.
  await page.reload();
  await page.getByLabel("Settings").click();
  await expect(
    page
      .getByRole("dialog")
      .getByRole("button", { name: "rebind Open Settings" }),
    // Renders "⌘⇧S" on macOS, "Ctrl+Shift+S" elsewhere — accept either.
  ).toHaveText(/(⇧S|Shift\+S)$/);
});

test("real tags: the Tags panel lists a tag and filtering shows its notes", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  // Fixtures: todo.md + projects/demo.md are tagged "rust".
  const rust = sidebar.getByRole("button", { name: "filter by tag rust" });
  await expect(rust).toBeVisible();
  await expect(rust).toHaveText(/rust\s*2/);

  // Click the tag → the results overlay lists the tagged notes.
  await rust.click();
  const overlay = page.getByTestId("search-results");
  await expect(overlay).toContainText("Tagged · rust");
  await expect(overlay.getByRole("button", { name: "todo.md" })).toBeVisible();
  await expect(
    overlay.getByRole("button", { name: "projects/demo.md" }),
  ).toBeVisible();

  // Opening a result closes the overlay.
  await overlay.getByRole("button", { name: "todo.md" }).click();
  await expect(page.getByTestId("search-results")).toHaveCount(0);
});

test("search shows a highlighted snippet", async ({ page }) => {
  await page.goto("/");
  await page.getByPlaceholder("Search…").fill("Start");
  await page.getByPlaceholder("Search…").press("Enter");
  const overlay = page.getByTestId("search-results");
  await expect(overlay).toBeVisible();
  // index.md ("Start at [[ideas]] …") matches; its row shows a snippet.
  await expect(overlay.getByRole("button", { name: "index.md" })).toContainText(
    "Start",
  );
  // the matched term is rendered as a <mark> highlight.
  await expect(overlay.locator("mark")).toContainText("Start");
  // clicking the result opens the note + closes the overlay.
  await overlay.getByRole("button", { name: "index.md" }).click();
  await expect(page.getByTestId("search-results")).toHaveCount(0);
});

test("rename: inline-rename a note in the tree", async ({ page }) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  const idx = sidebar.getByRole("button", { name: "index", exact: true });
  await expect(idx).toBeVisible();
  await idx.dblclick();
  const input = sidebar.getByRole("textbox");
  await input.fill("home");
  await input.press("Enter");
  await expect(
    sidebar.getByRole("button", { name: "home", exact: true }),
  ).toBeVisible();
  await expect(
    sidebar.getByRole("button", { name: "index", exact: true }),
  ).toHaveCount(0);
});

test("plugins: invoke a plugin command from the palette + see it in Settings", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await expect(sidebar.getByText("index", { exact: true })).toBeVisible();

  // Run the demo plugin command from the palette.
  await page.keyboard.press("Control+k");
  const input = page.getByPlaceholder(/type a command/i);
  await input.fill("stamp");
  await page.keyboard.press("Enter");

  // Side effect: a "stamp" note appears in the tree; a notice toast shows.
  await expect(
    sidebar.getByRole("button", { name: "stamp", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("stamp.md")).toBeVisible();

  // Settings shows the Plugins panel with the demo plugin.
  await page.getByLabel("Settings").click();
  await expect(page.getByRole("dialog").getByText(/Demo plugin/)).toBeVisible();
});
