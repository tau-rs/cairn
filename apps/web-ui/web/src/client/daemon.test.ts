import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  DaemonClient,
  DaemonHost,
  backoffDelay,
  RECONNECT_BASE_MS,
  RECONNECT_MAX_MS,
  ESCALATE_AFTER_ATTEMPTS,
  RECOVER_TIMEOUT_MS,
} from "./daemon";
import type { AnswerEvent } from "../contract";

// ── A controllable fake WebSocket ───────────────────────────────────────────
class FakeWebSocket {
  static instances: FakeWebSocket[] = [];
  onopen: ((e: unknown) => void) | null = null;
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onclose: ((e: unknown) => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  closed = false;
  sent: string[] = [];
  constructor(public url: string) {
    FakeWebSocket.instances.push(this);
  }
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.closed = true;
  }
  emitOpen() {
    this.onopen?.(undefined);
  }
  emitMessage(data: unknown) {
    this.onmessage?.({ data });
  }
  emitClose() {
    this.onclose?.(undefined);
  }
  static last() {
    return FakeWebSocket.instances[FakeWebSocket.instances.length - 1];
  }
  static reset() {
    FakeWebSocket.instances = [];
  }
}

const URL = "http://localhost:7777";
const WS = FakeWebSocket as unknown as { new (url: string): WebSocket };

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

beforeEach(() => FakeWebSocket.reset());

describe("DaemonClient HTTP", () => {
  it("sendCommand POSTs to /command with the bearer header and returns the response", async () => {
    const fetch = vi.fn().mockResolvedValue(jsonResponse({ type: "done" }));
    const c = new DaemonClient({
      url: URL,
      token: "secret",
      fetch,
      WebSocket: WS,
    });
    const res = await c.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "x",
    });
    expect(res).toEqual({ type: "done" });
    expect(fetch).toHaveBeenCalledWith("http://localhost:7777/command", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: "Bearer secret",
      },
      body: JSON.stringify({ type: "write_note", path: "a.md", contents: "x" }),
    });
  });

  it("runQuery POSTs to /query", async () => {
    const fetch = vi
      .fn()
      .mockResolvedValue(jsonResponse({ type: "paths", paths: ["a.md"] }));
    const c = new DaemonClient({
      url: URL,
      token: "secret",
      fetch,
      WebSocket: WS,
    });
    const res = await c.runQuery({ type: "get_backlinks", path: "a.md" });
    expect(res).toEqual({ type: "paths", paths: ["a.md"] });
    expect(fetch).toHaveBeenCalledWith(
      "http://localhost:7777/query",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("omits the Authorization header when no token is configured", async () => {
    const fetch = vi.fn().mockResolvedValue(jsonResponse({ type: "done" }));
    const c = new DaemonClient({ url: URL, fetch, WebSocket: WS });
    await c.sendCommand({ type: "commit", message: "m" });
    const headers = fetch.mock.calls[0][1].headers;
    expect(headers).not.toHaveProperty("Authorization");
  });

  it("rejects a ContractError body with that typed error (matching mock/tauri)", async () => {
    const err = { type: "not_found", what: "missing.md" };
    const fetch = vi.fn().mockResolvedValue(jsonResponse(err, 404));
    const c = new DaemonClient({ url: URL, fetch, WebSocket: WS });
    await expect(
      c.runQuery({ type: "get_note", path: "missing.md" }),
    ).rejects.toEqual(err);
  });

  it("rejects a 401 with an unauthorized error", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response("", { status: 401 }));
    const c = new DaemonClient({
      url: URL,
      token: "bad",
      fetch,
      WebSocket: WS,
    });
    await expect(c.runQuery({ type: "list_notes" })).rejects.toThrow(
      /unauthor/i,
    );
  });

  it("rejects a malformed 2xx body", async () => {
    const fetch = vi
      .fn()
      .mockResolvedValue(jsonResponse({ type: "not_a_real_response" }));
    const c = new DaemonClient({ url: URL, fetch, WebSocket: WS });
    await expect(c.runQuery({ type: "list_notes" })).rejects.toThrow(
      /Malformed query response/,
    );
  });

  it("noteTags derives tags from list_notes", async () => {
    const fetch = vi.fn().mockResolvedValue(
      jsonResponse({
        type: "notes",
        notes: [
          { path: "a.md", title: "A", tags: ["rust"] },
          { path: "b.md", title: "B", tags: [] },
        ],
      }),
    );
    const c = new DaemonClient({ url: URL, fetch, WebSocket: WS });
    expect(await c.noteTags()).toEqual({ "a.md": ["rust"], "b.md": [] });
  });
});

describe("DaemonClient subscribe", () => {
  it("connects to the ws:// events endpoint and forwards parsed events", () => {
    const c = new DaemonClient({ url: URL, fetch: vi.fn(), WebSocket: WS });
    const cb = vi.fn();
    c.subscribe(cb);
    const sock = FakeWebSocket.last();
    expect(sock.url).toBe("ws://localhost:7777/events");
    sock.emitMessage(JSON.stringify({ type: "committed", commit: "c1" }));
    expect(cb).toHaveBeenCalledWith({ type: "committed", commit: "c1" });
  });

  it("derives wss:// from an https daemon url", () => {
    const c = new DaemonClient({
      url: "https://cairn.example.com",
      fetch: vi.fn(),
      WebSocket: WS,
    });
    c.subscribe(vi.fn());
    expect(FakeWebSocket.last().url).toBe("wss://cairn.example.com/events");
  });

  it("routes a malformed frame to onError and never calls cb", () => {
    const c = new DaemonClient({ url: URL, fetch: vi.fn(), WebSocket: WS });
    const cb = vi.fn();
    const onError = vi.fn();
    c.subscribe(cb, onError);
    FakeWebSocket.last().emitMessage(JSON.stringify({ type: "garbage" }));
    expect(cb).not.toHaveBeenCalled();
    expect(String(onError.mock.calls[0][0])).toMatch(/Malformed event/);
  });

  it("unsubscribe closes the socket and stops reconnecting", () => {
    vi.useFakeTimers();
    const c = new DaemonClient({
      url: URL,
      fetch: vi.fn(),
      WebSocket: WS,
      random: () => 1,
    });
    const unsub = c.subscribe(vi.fn());
    const sock = FakeWebSocket.last();
    unsub();
    expect(sock.closed).toBe(true);
    sock.emitClose();
    vi.advanceTimersByTime(RECONNECT_MAX_MS);
    expect(FakeWebSocket.instances).toHaveLength(1); // no reconnect after unsub
    vi.useRealTimers();
  });

  describe("reconnect/backoff (B′)", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("silently reconnects on a drop without nagging on the first blip", () => {
      const onError = vi.fn();
      const c = new DaemonClient({
        url: URL,
        fetch: vi.fn(),
        WebSocket: WS,
        random: () => 1,
      });
      c.subscribe(vi.fn(), onError);
      FakeWebSocket.last().emitClose(); // first drop
      expect(onError).not.toHaveBeenCalled(); // no nag yet
      vi.advanceTimersByTime(backoffDelay(1));
      expect(FakeWebSocket.instances).toHaveLength(2); // reconnected
    });

    it("escalates to onError only after sustained failure", () => {
      const onError = vi.fn();
      const c = new DaemonClient({
        url: URL,
        fetch: vi.fn(),
        WebSocket: WS,
        random: () => 1,
      });
      c.subscribe(vi.fn(), onError);
      // Each reconnect immediately drops again → keep failing.
      for (let i = 1; i <= 4; i++) {
        FakeWebSocket.last().emitClose();
        vi.advanceTimersByTime(backoffDelay(i));
      }
      expect(onError).toHaveBeenCalled();
    });

    it("resets the backoff after a successful reconnect", () => {
      const c = new DaemonClient({
        url: URL,
        fetch: vi.fn(),
        WebSocket: WS,
        random: () => 1,
      });
      c.subscribe(vi.fn());
      FakeWebSocket.last().emitClose();
      vi.advanceTimersByTime(backoffDelay(1));
      FakeWebSocket.last().emitOpen(); // healed → attempt counter resets
      FakeWebSocket.last().emitClose(); // next drop starts from attempt 1 again
      vi.advanceTimersByTime(backoffDelay(1));
      expect(FakeWebSocket.instances).toHaveLength(3);
    });

    it("emits 'reconnecting' on a transient drop without erroring", () => {
      const onError = vi.fn();
      const onStatus = vi.fn();
      const c = new DaemonClient({
        url: URL,
        fetch: vi.fn(),
        WebSocket: WS,
        random: () => 1,
      });
      c.subscribe(vi.fn(), onError, onStatus);
      FakeWebSocket.last().emitClose(); // first drop → attempt 1
      expect(onStatus).toHaveBeenCalledWith("reconnecting");
      expect(onError).not.toHaveBeenCalled();
    });

    it("emits 'live' on a successful (re)open", () => {
      const onStatus = vi.fn();
      const c = new DaemonClient({
        url: URL,
        fetch: vi.fn(),
        WebSocket: WS,
        random: () => 1,
      });
      c.subscribe(vi.fn(), vi.fn(), onStatus);
      FakeWebSocket.last().emitClose();
      vi.advanceTimersByTime(backoffDelay(1));
      FakeWebSocket.last().emitOpen(); // healed
      expect(onStatus).toHaveBeenCalledWith("live");
    });

    it("stops emitting 'reconnecting' once escalated to onError (no downgrade)", () => {
      const onError = vi.fn();
      const onStatus = vi.fn();
      const c = new DaemonClient({
        url: URL,
        fetch: vi.fn(),
        WebSocket: WS,
        random: () => 1,
      });
      c.subscribe(vi.fn(), onError, onStatus);
      // Each reconnect immediately drops again → 5 failed attempts.
      for (let i = 1; i <= 5; i++) {
        FakeWebSocket.last().emitClose();
        vi.advanceTimersByTime(backoffDelay(i));
      }
      expect(onError).toHaveBeenCalled();
      // Pill only for attempts 1–(ESCALATE_AFTER_ATTEMPTS-1); remaining attempts escalate via onError instead.
      const reconnecting = onStatus.mock.calls.filter(
        (a) => a[0] === "reconnecting",
      );
      expect(reconnecting).toHaveLength(ESCALATE_AFTER_ATTEMPTS - 1);
    });
  });
});

describe("DaemonClient.openRecovery", () => {
  it("puts the token on the /collab URL as a ?token= query param (a WS handshake can't send an Authorization header, and /collab is token-gated)", () => {
    const client = new DaemonClient({
      url: URL,
      token: "s3cr3t/tok",
      fetch: vi.fn(),
      WebSocket: WS,
    });
    void client.openRecovery("draft.md");
    expect(FakeWebSocket.last().url).toBe(
      "ws://localhost:7777/collab?token=s3cr3t%2Ftok",
    );
  });

  it("joins /collab, returns recoverable blocks, restore awaits op", async () => {
    const client = new DaemonClient({
      url: URL,
      fetch: vi.fn(),
      WebSocket: WS,
    });
    const p = client.openRecovery("draft.md");
    const ws = FakeWebSocket.last();
    expect(ws.url).toBe("ws://localhost:7777/collab");

    ws.emitOpen();
    expect(JSON.parse(ws.sent[0]).type).toBe("join");
    expect(ws.sent.some((m) => JSON.parse(m).type === "recover")).toBe(true);

    ws.emitMessage(
      JSON.stringify({
        type: "recoverable",
        note: "draft.md",
        blocks: [
          {
            id: { replica: "1", counter: "2" },
            tombstoned: true,
            versions: ["x"],
          },
        ],
      }),
    );
    const session = await p;
    expect(session.blocks.length).toBe(1);

    const rp = session.restore(session.blocks[0].id, 0);
    expect(ws.sent.some((m) => JSON.parse(m).type === "restore")).toBe(true);
    ws.emitMessage(
      JSON.stringify({ type: "op", note: "draft.md", op: { op: "insert" } }),
    );
    await rp;
  });

  it("round-trips a seed-replica (u64::MAX) block id through restore without precision loss", async () => {
    // The bug this guards: the daemon's seed replica is DAEMON_REPLICA
    // (u64::MAX ≈ 1.8e19). It used to ride the wire as a bare JSON number, so
    // JSON.parse rounded it above 2^53 and the echoed Restore no longer matched
    // the real block. The engine now string-encodes every u64, so the id must
    // survive parse→restore byte-for-byte as an opaque string.
    const SEED_ID = { replica: "18446744073709551615", counter: "7" };
    const client = new DaemonClient({
      url: URL,
      fetch: vi.fn(),
      WebSocket: WS,
    });
    const p = client.openRecovery("draft.md");
    const ws = FakeWebSocket.last();

    ws.emitOpen();
    // The client's own replica also rides the wire as a decimal string now.
    expect(typeof JSON.parse(ws.sent[0]).replica).toBe("string");

    ws.emitMessage(
      JSON.stringify({
        type: "recoverable",
        note: "draft.md",
        blocks: [{ id: SEED_ID, tombstoned: true, versions: ["x"] }],
      }),
    );
    const session = await p;
    expect(session.blocks[0].id).toEqual(SEED_ID);

    session.restore(session.blocks[0].id, 0);
    const restoreMsg = ws.sent
      .map((m) => JSON.parse(m))
      .find((m) => m.type === "restore");
    // The exact u64::MAX string must reach the daemon untouched — not rounded,
    // not re-encoded as a number.
    expect(restoreMsg.id).toEqual(SEED_ID);
    expect(restoreMsg.id.replica).toBe("18446744073709551615");
  });

  it("rejects if the socket closes before recoverable arrives", async () => {
    const client = new DaemonClient({
      url: URL,
      fetch: vi.fn(),
      WebSocket: WS,
    });
    const p = client.openRecovery("draft.md");
    FakeWebSocket.last().emitClose();
    await expect(p).rejects.toThrow(/collab closed/);
  });

  it("close() sends leave and closes the socket", async () => {
    const client = new DaemonClient({
      url: URL,
      fetch: vi.fn(),
      WebSocket: WS,
    });
    const p = client.openRecovery("draft.md");
    const ws = FakeWebSocket.last();
    ws.emitOpen();
    ws.emitMessage(
      JSON.stringify({ type: "recoverable", note: "draft.md", blocks: [] }),
    );
    const session = await p;
    session.close();
    expect(ws.sent.some((m) => JSON.parse(m).type === "leave")).toBe(true);
    expect(ws.closed).toBe(true);
  });

  it("close() drains a pending restore() so it resolves instead of hanging", async () => {
    const client = new DaemonClient({
      url: URL,
      fetch: vi.fn(),
      WebSocket: WS,
    });
    const p = client.openRecovery("draft.md");
    const ws = FakeWebSocket.last();
    ws.emitOpen();
    ws.emitMessage(
      JSON.stringify({
        type: "recoverable",
        note: "draft.md",
        blocks: [
          {
            id: { replica: "1", counter: "2" },
            tombstoned: true,
            versions: ["x"],
          },
        ],
      }),
    );
    const session = await p;

    const rp = session.restore(session.blocks[0].id, 0);
    // No `op` is ever emitted; close() must still settle rp without relying
    // on the 5s fallback timer.
    session.close();
    await rp;
  });

  it("times out and closes the socket if recoverable never arrives", async () => {
    vi.useFakeTimers();
    try {
      const client = new DaemonClient({
        url: URL,
        fetch: vi.fn(),
        WebSocket: WS,
      });
      const p = client.openRecovery("draft.md");
      const ws = FakeWebSocket.last();
      ws.emitOpen();
      // No `recoverable` (or any) message ever arrives.
      const assertion = expect(p).rejects.toThrow(/timed out/);
      await vi.advanceTimersByTimeAsync(RECOVER_TIMEOUT_MS);
      await assertion;
      expect(ws.closed).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

class CollabFakeWS {
  static last: CollabFakeWS | null = null;
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];
  closed = false;
  constructor(public url: string) {
    CollabFakeWS.last = this;
  }
  send(s: string) {
    this.sent.push(s);
  }
  close() {
    this.closed = true;
  }
  // test helpers
  open() {
    this.onopen?.();
  }
  message(obj: unknown) {
    this.onmessage?.({ data: JSON.stringify(obj) });
  }
}

describe("DaemonClient.openCollab", () => {
  it("openCollab joins, routes frames, and leaves on close", () => {
    const client = new DaemonClient({
      url: "http://d",
      token: "t0",
      WebSocket: CollabFakeWS as unknown as { new (u: string): WebSocket },
      random: () => 0.5,
    });
    const ops: string[] = [];
    let snap = 0;
    const session = client.openCollab("n.md", {
      onSnapshot: () => (snap += 1),
      onForeignOp: (note) => ops.push(note),
    });
    const ws = CollabFakeWS.last!;
    expect(ws.url).toBe("ws://d/collab?token=t0"); // token-gated query param

    ws.open();
    expect(JSON.parse(ws.sent[0])).toMatchObject({
      type: "join",
      note: "n.md",
    });

    ws.message({ type: "snapshot", note: "n.md", ops: [] });
    expect(snap).toBe(1);
    ws.message({
      type: "op",
      note: "n.md",
      op: { op: "delete", id: { replica: "1", counter: "2" }, lamport: "5" },
    });
    expect(ops).toEqual(["n.md"]);
    // A frame for a different note is ignored.
    ws.message({
      type: "op",
      note: "other.md",
      op: { op: "delete", id: { replica: "1", counter: "3" }, lamport: "6" },
    });
    expect(ops).toEqual(["n.md"]);
    // A malformed frame is dropped, not thrown.
    expect(() => ws.message({ type: "bogus" })).not.toThrow();

    session.close();
    expect(JSON.parse(ws.sent[1])).toMatchObject({
      type: "leave",
      note: "n.md",
    });
    expect(ws.closed).toBe(true);
    session.close(); // idempotent
    expect(ws.sent.length).toBe(2);
  });
});

describe("backoffDelay", () => {
  it("doubles from the base and caps at the max", () => {
    expect(backoffDelay(1)).toBe(RECONNECT_BASE_MS);
    expect(backoffDelay(2)).toBe(RECONNECT_BASE_MS * 2);
    expect(backoffDelay(3)).toBe(RECONNECT_BASE_MS * 4);
    expect(backoffDelay(99)).toBe(RECONNECT_MAX_MS);
  });
});

describe("DaemonClient.ask", () => {
  function sseResponse(chunks: string[], init?: ResponseInit): Response {
    const enc = new TextEncoder();
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const c of chunks) controller.enqueue(enc.encode(c));
        controller.close();
      },
    });
    return new Response(stream, {
      status: 200,
      headers: { "content-type": "text/event-stream" },
      ...init,
    });
  }
  function frame(e: AnswerEvent): string {
    return `data: ${JSON.stringify(e)}\n\n`;
  }
  function collect(
    client: DaemonClient,
    onError?: (e: unknown) => void,
  ): Promise<AnswerEvent[]> {
    return new Promise((resolve) => {
      const got: AnswerEvent[] = [];
      client.ask(
        { query: "q", top_k: null },
        (e) => {
          got.push(e);
          if (e.type === "completed" || e.type === "failed") resolve(got);
        },
        (err) => {
          onError?.(err);
          resolve(got);
        },
      );
    });
  }

  it("yields sources -> deltas -> completed in order and POSTs the request", async () => {
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: ["a.md", "b.md"] }),
        frame({ type: "text_delta", text: "Hello " }),
        frame({ type: "text_delta", text: "world" }),
        frame({ type: "completed" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const client = new DaemonClient({ url: "http://x", token: "t", fetch });
    const got = await collect(client);
    expect(got.map((e) => e.type)).toEqual([
      "sources",
      "text_delta",
      "text_delta",
      "completed",
    ]);
    expect(got[0]).toEqual({ type: "sources", paths: ["a.md", "b.md"] });
    const [url, opts] = (fetch as unknown as ReturnType<typeof vi.fn>).mock
      .calls[0];
    expect(url).toBe("http://x/ask");
    expect(opts.method).toBe("POST");
    expect(JSON.parse(opts.body)).toEqual({ query: "q", top_k: null });
    expect(opts.headers.Authorization).toBe("Bearer t");
  });

  it("parses multiple frames in one chunk", async () => {
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: [] }) +
          frame({ type: "text_delta", text: "x" }) +
          frame({ type: "completed" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const got = await collect(new DaemonClient({ url: "http://x", fetch }));
    expect(got.map((e) => e.type)).toEqual([
      "sources",
      "text_delta",
      "completed",
    ]);
  });

  it("parses a frame split across chunks", async () => {
    const full = frame({ type: "text_delta", text: "hi" });
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: [] }),
        full.slice(0, 6),
        full.slice(6),
        frame({ type: "completed" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const got = await collect(new DaemonClient({ url: "http://x", fetch }));
    expect(got).toContainEqual({ type: "text_delta", text: "hi" });
  });

  it("delivers an in-run failed frame as an event, not onError", async () => {
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: [] }),
        frame({ type: "failed", message: "boom" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const onError = vi.fn();
    const got = await collect(
      new DaemonClient({ url: "http://x", fetch }),
      onError,
    );
    expect(got[got.length - 1]).toEqual({ type: "failed", message: "boom" });
    expect(onError).not.toHaveBeenCalled();
  });

  it("routes a pre-stream HTTP error to onError", async () => {
    const fetch = vi.fn(
      async () => new Response("nope", { status: 401 }),
    ) as unknown as typeof globalThis.fetch;
    const onError = vi.fn();
    await collect(new DaemonClient({ url: "http://x", fetch }), onError);
    expect(onError).toHaveBeenCalledOnce();
    expect(String((onError.mock.calls[0][0] as Error).message)).toMatch(
      /unauthorized/,
    );
  });

  it("stops emitting after unsubscribe and cancels the reader", async () => {
    let cancelled = false;
    const enc = new TextEncoder();
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(
          enc.encode(
            `data: ${JSON.stringify({ type: "sources", paths: [] })}\n\n`,
          ),
        );
        // never closes; the test cancels.
      },
      cancel() {
        cancelled = true;
      },
    });
    const fetch = vi.fn(
      async () =>
        new Response(stream, {
          status: 200,
          headers: { "content-type": "text/event-stream" },
        }),
    ) as unknown as typeof globalThis.fetch;
    const seen: AnswerEvent[] = [];
    const unsub = new DaemonClient({ url: "http://x", fetch }).ask(
      { query: "q", top_k: null },
      (e) => seen.push(e),
    );
    await new Promise((r) => setTimeout(r, 10));
    unsub();
    await new Promise((r) => setTimeout(r, 10));
    expect(cancelled).toBe(true);
    expect(seen.every((e) => e.type === "sources")).toBe(true);
  });
});

describe("DaemonHost", () => {
  it("reports the daemon url as the open cairn", async () => {
    const h = new DaemonHost(URL);
    expect(await h.currentCairn()).toBe(URL);
    expect(await h.openCairn()).toBe(URL);
  });

  it("passes remote and data urls through but returns local paths unresolved", () => {
    const h = new DaemonHost(URL);
    expect(h.assetUrl("https://x/y.png")).toBe("https://x/y.png");
    expect(h.assetUrl("data:image/png;base64,AAAA")).toBe(
      "data:image/png;base64,AAAA",
    );
    expect(h.assetUrl("img/local.png")).toBe("img/local.png");
  });
});

describe("DaemonClient — hardening (mutation)", () => {
  function sse(chunks: string[], init?: ResponseInit): Response {
    const enc = new TextEncoder();
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const c of chunks) controller.enqueue(enc.encode(c));
        controller.close();
      },
    });
    return new Response(stream, {
      status: 200,
      headers: { "content-type": "text/event-stream" },
      ...init,
    });
  }

  it("maps a non-401 error status to a generic message", async () => {
    // Guards the `res.status === 401` branch AND the `${res.status}` interpolation.
    const fetch = vi.fn(async () =>
      jsonResponse({ not: "a contract error" }, 503),
    ) as unknown as typeof globalThis.fetch;
    const client = new DaemonClient({ url: URL, fetch });
    await expect(client.runQuery({ type: "list_notes" })).rejects.toThrow(
      "daemon request failed: 503",
    );
  });

  it("coerces non-string WebSocket frame data via String() before parsing", () => {
    // Guards `typeof ev.data === "string" ? ev.data : String(ev.data)`: some
    // WS impls deliver a non-string; it must still be stringified and parsed.
    const c = new DaemonClient({ url: URL, fetch: vi.fn(), WebSocket: WS });
    const cb = vi.fn();
    c.subscribe(cb);
    FakeWebSocket.last().emitMessage({
      toString: () => JSON.stringify({ type: "committed", commit: "c9" }),
    });
    expect(cb).toHaveBeenCalledWith({ type: "committed", commit: "c9" });
  });

  it("ask() errors when the response has no body stream", async () => {
    const fetch = vi.fn(
      async () =>
        new Response(null, {
          status: 200,
          headers: { "content-type": "text/event-stream" },
        }),
    ) as unknown as typeof globalThis.fetch;
    const client = new DaemonClient({ url: URL, fetch });
    const err = await new Promise<unknown>((resolve) => {
      client.ask({ query: "q", top_k: null }, () => {}, resolve);
    });
    expect(String(err)).toMatch(/empty stream/);
  });

  it("ask() parses a trailing frame that has no blank-line terminator", async () => {
    // Guards the post-loop flush (`decoder.decode()` + tail `parseSseFrame`).
    const fetch = vi.fn(async () =>
      sse([`data: ${JSON.stringify({ type: "completed" })}`]),
    ) as unknown as typeof globalThis.fetch;
    const client = new DaemonClient({ url: URL, fetch });
    const got = await new Promise<AnswerEvent[]>((resolve) => {
      const out: AnswerEvent[] = [];
      client.ask(
        { query: "q", top_k: null },
        (e) => {
          out.push(e);
          if (e.type === "completed") resolve(out);
        },
        () => resolve(out),
      );
    });
    expect(got.map((e) => e.type)).toEqual(["completed"]);
  });

  it("ask() ignores comment/event lines and reads only `data:`", async () => {
    // Guards parseSseFrame's `startsWith("data:")` filter and the ` `-trim.
    const fetch = vi.fn(async () =>
      sse([
        `event: message\n: a comment\ndata: ${JSON.stringify({ type: "completed" })}\n\n`,
      ]),
    ) as unknown as typeof globalThis.fetch;
    const client = new DaemonClient({ url: URL, fetch });
    const got = await new Promise<AnswerEvent[]>((resolve) => {
      const out: AnswerEvent[] = [];
      client.ask(
        { query: "q", top_k: null },
        (e) => {
          out.push(e);
          if (e.type === "completed") resolve(out);
        },
        () => resolve(out),
      );
    });
    expect(got.map((e) => e.type)).toEqual(["completed"]);
  });

  it("openRecovery ignores a `recoverable` for a different note", async () => {
    // Guards `msg.note === note`: a recoverable for another note must not
    // resolve THIS session with the wrong blocks.
    const client = new DaemonClient({
      url: URL,
      fetch: vi.fn(),
      WebSocket: WS,
    });
    const p = client.openRecovery("draft.md");
    const ws = FakeWebSocket.last();
    ws.emitOpen();
    ws.emitMessage(
      JSON.stringify({
        type: "recoverable",
        note: "other.md",
        blocks: [
          {
            id: { replica: "9", counter: "9" },
            tombstoned: false,
            versions: ["X"],
          },
        ],
      }),
    );
    ws.emitMessage(
      JSON.stringify({ type: "recoverable", note: "draft.md", blocks: [] }),
    );
    const session = await p;
    expect(session.blocks).toEqual([]); // draft.md's blocks, not other.md's
  });
});
