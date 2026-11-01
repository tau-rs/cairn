import { describe, it, expect, beforeEach } from "vitest";
import { loadDaemonConfig, saveDaemonConfig } from "./daemonConfig";

describe("daemonConfig", () => {
  beforeEach(() => localStorage.clear());

  it("returns empty config when nothing is stored", () => {
    expect(loadDaemonConfig()).toEqual({ url: "", token: "" });
  });

  it("round-trips a saved config", () => {
    saveDaemonConfig({ url: "http://localhost:7777", token: "abc123" });
    expect(loadDaemonConfig()).toEqual({
      url: "http://localhost:7777",
      token: "abc123",
    });
  });

  it("tolerates malformed JSON and returns the empty config", () => {
    localStorage.setItem("cairn.daemon", "{not json");
    expect(loadDaemonConfig()).toEqual({ url: "", token: "" });
  });

  it("defaults missing fields rather than returning undefined", () => {
    localStorage.setItem("cairn.daemon", JSON.stringify({ url: "http://x" }));
    expect(loadDaemonConfig()).toEqual({ url: "http://x", token: "" });
  });

  it("coerces non-string fields to empty strings", () => {
    localStorage.setItem(
      "cairn.daemon",
      JSON.stringify({ url: 42, token: null }),
    );
    expect(loadDaemonConfig()).toEqual({ url: "", token: "" });
  });
});
