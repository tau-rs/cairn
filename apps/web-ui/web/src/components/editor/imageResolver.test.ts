import { describe, it, expect, vi } from "vitest";
import { makeImageResolver } from "./imageResolver";

describe("makeImageResolver", () => {
  it("blocks http(s) URLs by default (no opt-in)", () => {
    const assetUrl = vi.fn();
    const r = makeImageResolver(assetUrl);
    expect(r("https://x/y.png")).toEqual({
      kind: "blocked",
      src: "https://x/y.png",
    });
    expect(assetUrl).not.toHaveBeenCalled();
  });
  it("blocks data URLs by default", () => {
    const r = makeImageResolver(vi.fn());
    expect(r("data:image/png;base64,AAAA")).toEqual({
      kind: "blocked",
      src: "data:image/png;base64,AAAA",
    });
  });
  it("passes remote URLs through as ready when loadRemote is on", () => {
    const r = makeImageResolver(vi.fn(), { loadRemote: true });
    expect(r("https://x/y.png")).toEqual({
      kind: "ready",
      url: "https://x/y.png",
    });
  });
  it("resolves local relative paths via assetUrl regardless of loadRemote", () => {
    const assetUrl = vi.fn().mockReturnValue("asset://img/logo.png");
    const r = makeImageResolver(assetUrl);
    expect(r("img/logo.png")).toEqual({
      kind: "ready",
      url: "asset://img/logo.png",
    });
    expect(assetUrl).toHaveBeenCalledWith("img/logo.png");
  });
  it("passes a remote URL through as ready once individually approved", () => {
    const approved = new Set(["https://x/y.png"]);
    const r = makeImageResolver(vi.fn(), {
      isApproved: (src) => approved.has(src),
    });
    expect(r("https://x/y.png")).toEqual({
      kind: "ready",
      url: "https://x/y.png",
    });
    expect(r("https://x/other.png")).toEqual({
      kind: "blocked",
      src: "https://x/other.png",
    });
  });
  it("marks a local path the host refuses (empty url) as invalid, never ready", () => {
    // The host (TauriHost.assetUrl) returns "" when a path escapes the vault.
    const assetUrl = vi.fn().mockReturnValue("");
    const r = makeImageResolver(assetUrl);
    expect(r("../../etc/passwd")).toEqual({
      kind: "invalid",
      src: "../../etc/passwd",
    });
  });
});
