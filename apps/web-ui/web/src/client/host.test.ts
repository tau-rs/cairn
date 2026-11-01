import { describe, it, expect } from "vitest";
import { alwaysOpenHost, MockHost } from "./host";

describe("alwaysOpenHost", () => {
  it("reports a cairn always open", async () => {
    expect(await alwaysOpenHost.currentCairn()).toBe("(fixture)");
    expect(await alwaysOpenHost.openCairn()).toBe("(fixture)");
  });
});

describe("MockHost", () => {
  it("behaves like the always-open host", async () => {
    const h = new MockHost();
    expect(await h.currentCairn()).toBe("(fixture)");
    expect(await h.openCairn()).toBe("(fixture)");
  });
});

describe("MockHost.assetUrl", () => {
  it("returns an inline data image for any local path", () => {
    const url = new MockHost().assetUrl("img/logo.png");
    expect(url.startsWith("data:image/")).toBe(true);
  });
});

describe("MockHost.setPluginUiRoots", () => {
  it("resolves to undefined (no-op off Tauri)", async () => {
    expect(await new MockHost().setPluginUiRoots({ wc: "/tmp/wc/ui" })).toBe(
      undefined,
    );
  });
});
