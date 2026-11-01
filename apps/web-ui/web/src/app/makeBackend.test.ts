import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));

import { makeBackend } from "./makeBackend";
import { MockClient } from "../client/mock";
import { MockHost } from "../client/host";
import { DaemonClient, DaemonHost } from "../client/daemon";

describe("makeBackend", () => {
  beforeEach(() => localStorage.clear());

  it("returns the mock backend when not under Tauri and no daemon is configured", () => {
    const { client, host } = makeBackend();
    expect(client).toBeInstanceOf(MockClient);
    expect(host).toBeInstanceOf(MockHost);
  });

  it("returns the daemon backend when a daemon url is configured (non-Tauri)", () => {
    localStorage.setItem(
      "cairn.daemon",
      JSON.stringify({ url: "http://localhost:7777", token: "t" }),
    );
    const { client, host } = makeBackend();
    expect(client).toBeInstanceOf(DaemonClient);
    expect(host).toBeInstanceOf(DaemonHost);
  });

  it("falls back to the mock when the configured daemon url is blank", () => {
    localStorage.setItem(
      "cairn.daemon",
      JSON.stringify({ url: "", token: "" }),
    );
    const { client } = makeBackend();
    expect(client).toBeInstanceOf(MockClient);
  });
});
