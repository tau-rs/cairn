import { afterEach, describe, expect, it, vi } from "vitest";
import { createBroker } from "./pluginBroker";
import { MAX_BROKER_STR } from "./pluginTier3";
import type { BrokerHost } from "./pluginBrokerHost";

function host(over: Partial<BrokerHost> = {}): BrokerHost {
  return {
    info: () => ({ appVersion: "1", theme: "dark" }),
    notice: vi.fn(),
    activeNote: () => ({ path: "a.md", title: "a", text: "hi" }),
    writeActiveNote: vi.fn(),
    readNote: vi.fn(async () => ({ path: "b.md", text: "B" })),
    search: vi.fn(async () => [{ path: "a.md" }]),
    invokeOwnCommand: vi.fn(async () => {}),
    subscribeActiveNote: () => () => {},
    ...over,
  };
}

// A fake iframe window: messages the broker posts back land in `sent`.
function fakeFrame() {
  const sent: unknown[] = [];
  const win = {
    postMessage: (m: unknown) => sent.push(m),
  } as unknown as Window;
  return { win, sent };
}

function send(source: Window, data: unknown) {
  window.dispatchEvent(new MessageEvent("message", { source, data }));
}

let teardown: (() => void) | null = null;
afterEach(() => {
  teardown?.();
  teardown = null;
});

describe("plugin broker", () => {
  it("ignores messages from a foreign source", async () => {
    const h = host();
    const { win } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(),
      pluginCommands: new Set(),
      host: h,
    });
    teardown = b.dispose;
    const other = { postMessage: vi.fn() } as unknown as Window;
    send(other, { t: "req", id: "1", method: "host.info" });
    await Promise.resolve();
    expect(h.notice).not.toHaveBeenCalled();
  });

  it("answers a silent method (host.info) without a grant", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(),
      pluginCommands: new Set(),
      host: h,
    });
    teardown = b.dispose;
    send(win, { t: "req", id: "1", method: "host.info" });
    await vi.waitFor(() =>
      expect(sent).toContainEqual({
        t: "res",
        id: "1",
        ok: true,
        result: { appVersion: "1", theme: "dark" },
      }),
    );
  });

  it("denies a permissioned method without the grant", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(),
      pluginCommands: new Set(),
      host: h,
    });
    teardown = b.dispose;
    send(win, {
      t: "req",
      id: "1",
      method: "activeNote.write",
      params: { text: "x" },
    });
    await vi.waitFor(() =>
      expect(sent).toContainEqual({
        t: "res",
        id: "1",
        ok: false,
        error: "denied",
      }),
    );
    expect(h.writeActiveNote).not.toHaveBeenCalled();
  });

  it("allows a granted method", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(["activeNote.write"]),
      pluginCommands: new Set(),
      host: h,
    });
    teardown = b.dispose;
    send(win, {
      t: "req",
      id: "1",
      method: "activeNote.write",
      params: { text: "new" },
    });
    await vi.waitFor(() =>
      expect(h.writeActiveNote).toHaveBeenCalledWith("new"),
    );
    expect(sent).toContainEqual({ t: "res", id: "1", ok: true, result: null });
  });

  it("confines command.invoke to the plugin's own commands", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(["command.invoke"]),
      pluginCommands: new Set(["mine"]),
      host: h,
    });
    teardown = b.dispose;
    send(win, {
      t: "req",
      id: "1",
      method: "command.invoke",
      params: { command: "notMine" },
    });
    await vi.waitFor(() =>
      expect(sent).toContainEqual({
        t: "res",
        id: "1",
        ok: false,
        error: "unknown command",
      }),
    );
    expect(h.invokeOwnCommand).not.toHaveBeenCalled();
  });

  it("drops messages beyond the inbound rate cap", async () => {
    const h = host({
      info: vi.fn(() => ({ appVersion: "1", theme: "dark" })),
    });
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(),
      pluginCommands: new Set(),
      host: h,
      rateMax: 3,
    });
    teardown = b.dispose;
    for (let i = 0; i < 10; i++)
      send(win, { t: "req", id: String(i), method: "host.info" });
    await Promise.resolve();
    expect(
      (sent as Array<{ ok: boolean }>).filter((m) => m.ok).length,
    ).toBeLessThanOrEqual(3);
  });

  it("rejects a hanging request with a timeout", async () => {
    const h = host({ readNote: () => new Promise(() => {}) }); // never resolves
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(["notes.read"]),
      pluginCommands: new Set(),
      host: h,
      requestTimeoutMs: 5,
    });
    teardown = b.dispose;
    send(win, {
      t: "req",
      id: "1",
      method: "notes.read",
      params: { path: "x.md" },
    });
    await vi.waitFor(() =>
      expect(sent).toContainEqual({
        t: "res",
        id: "1",
        ok: false,
        error: "timeout",
      }),
    );
  });

  it("silently drops malformed (non-req) messages", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(),
      pluginCommands: new Set(),
      host: h,
    });
    teardown = b.dispose;
    send(win, { t: "req", method: "host.info" }); // missing id
    send(win, { t: "notify", id: "1", method: "host.info" }); // wrong tag
    send(win, "not even an object");
    await Promise.resolve();
    expect(sent).toHaveLength(0);
    expect(h.notice).not.toHaveBeenCalled();
  });

  it("replies 'unknown method' for an unrecognized method", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(),
      pluginCommands: new Set(),
      host: h,
    });
    teardown = b.dispose;
    send(win, { t: "req", id: "1", method: "filesystem.format" });
    await vi.waitFor(() =>
      expect(sent).toContainEqual({
        t: "res",
        id: "1",
        ok: false,
        error: "unknown method",
      }),
    );
  });

  it("clamps oversized string params before dispatching to the host", async () => {
    const writeActiveNote = vi.fn();
    const h = host({ writeActiveNote });
    const { win } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(["activeNote.write"]),
      pluginCommands: new Set(),
      host: h,
    });
    teardown = b.dispose;
    send(win, {
      t: "req",
      id: "1",
      method: "activeNote.write",
      params: { text: "x".repeat(MAX_BROKER_STR + 50) },
    });
    await vi.waitFor(() => expect(writeActiveNote).toHaveBeenCalled());
    expect(writeActiveNote.mock.calls[0][0]).toHaveLength(MAX_BROKER_STR);
  });

  it("ignores frame messages after dispose", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(),
      pluginCommands: new Set(),
      host: h,
    });
    b.dispose();
    teardown = null;
    send(win, { t: "req", id: "1", method: "host.info" });
    await Promise.resolve();
    expect(sent).toHaveLength(0);
  });
});
