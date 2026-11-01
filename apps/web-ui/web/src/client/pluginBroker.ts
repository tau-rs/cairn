// Per-frame postMessage broker. The single wall between an untrusted sandboxed
// iframe and the host: authenticate by event.source, rate-cap, capability-gate,
// clamp params, dispatch to the BrokerHost port, reply within a timeout.
import {
  BROKER_RATE_MAX,
  BROKER_RATE_WINDOW_MS,
  BROKER_REQUEST_TIMEOUT_MS,
  CAPABILITY_OF,
  MAX_BROKER_STR,
  type BrokerMethod,
  type PluginCapability,
} from "./pluginTier3";
import type { BrokerHost } from "./pluginBrokerHost";
import type { JsonValue } from "../contract/serde_json/JsonValue";

export type BrokerOptions = {
  frame: Window;
  plugin: string;
  granted: ReadonlySet<PluginCapability>;
  pluginCommands: ReadonlySet<string>;
  host: BrokerHost;
  rateMax?: number;
  requestTimeoutMs?: number;
};

type Req = { t: "req"; id: string; method: string; params?: unknown };

const METHODS = new Set<string>(Object.keys(CAPABILITY_OF));

function isReq(x: unknown): x is Req {
  if (typeof x !== "object" || x === null) return false;
  const r = x as Record<string, unknown>;
  return (
    r.t === "req" && typeof r.id === "string" && typeof r.method === "string"
  );
}

function clampText(x: unknown): string {
  if (typeof x !== "string") return "";
  return x.length > MAX_BROKER_STR ? x.slice(0, MAX_BROKER_STR) : x;
}

export function createBroker(opts: BrokerOptions): { dispose: () => void } {
  const { frame, plugin, granted, pluginCommands, host } = opts;
  const rateMax = opts.rateMax ?? BROKER_RATE_MAX;
  const requestTimeoutMs = opts.requestTimeoutMs ?? BROKER_REQUEST_TIMEOUT_MS;

  // Inbound rate cap: at most `rateMax` accepted messages per rolling window.
  // Window resets on a timer, so a synchronous flood only lets `rateMax` through.
  let inWindow = 0;
  let windowTimer: ReturnType<typeof setTimeout> | null = null;
  function withinRateCap(): boolean {
    if (windowTimer === null) {
      windowTimer = setTimeout(() => {
        inWindow = 0;
        windowTimer = null;
      }, BROKER_RATE_WINDOW_MS);
    }
    inWindow += 1;
    return inWindow <= rateMax;
  }

  // targetOrigin MUST be "*": the sandbox frame is null/opaque origin, so any
  // concrete targetOrigin (or the "/" default of the 1-arg form) would cause the
  // browser to silently drop the reply. The frame is authenticated inbound by
  // event.source, not by an origin string.
  const reply = (
    id: string,
    body: { ok: true; result: JsonValue } | { ok: false; error: string },
  ) => frame.postMessage({ t: "res", id, ...body }, "*");

  // Per-request timeout: a hanging host method rejects with "timeout" instead of
  // leaving the plugin (and its pending request) stuck forever (spec §8.2).
  const withTimeout = (p: Promise<JsonValue>): Promise<JsonValue> =>
    Promise.race([
      p,
      new Promise<JsonValue>((_, rej) =>
        setTimeout(() => rej(new Error("timeout")), requestTimeoutMs),
      ),
    ]);

  // Dispatch a method. Synchronous methods return a value directly (their reply
  // fires in the same tick); the three async methods return a Promise, which the
  // caller routes through withTimeout. The union return type is the signal of
  // which path a method takes — the caller branches on `instanceof Promise`.
  const dispatch = (
    method: BrokerMethod,
    params: Record<string, unknown>,
  ): JsonValue | Promise<JsonValue> => {
    switch (method) {
      case "host.info":
        return host.info();
      case "ui.notice":
        host.notice(clampText(params.text));
        return null;
      case "activeNote.read":
        return host.activeNote();
      case "activeNote.subscribe":
        // Subscription wiring is owned by IframeHost (it forwards events); the
        // broker just acknowledges the subscribe request.
        return null;
      case "activeNote.write":
        host.writeActiveNote(clampText(params.text));
        return null;
      case "notes.read":
        return host.readNote(clampText(params.path));
      case "notes.search":
        return host.search(clampText(params.query));
      case "command.invoke": {
        const command = clampText(params.command);
        if (!pluginCommands.has(command)) throw new Error("unknown command");
        // `args` is untrusted and passed through unvalidated; the host's command
        // runner is responsible for validating its own command arguments.
        return host
          .invokeOwnCommand(plugin, command, (params.args as JsonValue) ?? null)
          .then(() => null as JsonValue);
      }
      default:
        throw new Error("unknown method");
    }
  };

  const onMessage = (e: MessageEvent) => {
    if (e.source !== frame) return; // 1. identity (origin is null → use source)
    const msg = e.data as unknown;
    if (!isReq(msg)) return; // 2. shape-validate, drop malformed
    // Internal protocol messages (e.g. "__handshake") are owned by IframeHost,
    // not the broker. Ignore them here so they neither burn a rate slot nor draw
    // a spurious "unknown method" reply.
    if (msg.method.startsWith("__")) return;
    if (!withinRateCap()) return; // 3. inbound rate cap (flood guard)

    if (!METHODS.has(msg.method)) {
      reply(msg.id, { ok: false, error: "unknown method" });
      return;
    }
    const method = msg.method as BrokerMethod;
    const cap = CAPABILITY_OF[method];
    if (cap && !granted.has(cap)) {
      reply(msg.id, { ok: false, error: "denied" }); // 4. broker is the wall
      return;
    }
    const params = (
      typeof msg.params === "object" && msg.params !== null ? msg.params : {}
    ) as Record<string, unknown>;

    let result: JsonValue | Promise<JsonValue>;
    try {
      result = dispatch(method, params);
    } catch (err) {
      reply(msg.id, {
        ok: false,
        error: err instanceof Error ? err.message : "error",
      });
      return;
    }

    // 5. If sync, reply immediately. If async, race against a timeout.
    // Never let a rejection escape.
    if (!(result instanceof Promise)) {
      reply(msg.id, { ok: true, result });
      return;
    }
    void withTimeout(result).then(
      (res) => reply(msg.id, { ok: true, result: res }),
      (err: unknown) =>
        reply(msg.id, {
          ok: false,
          error: err instanceof Error ? err.message : "error",
        }),
    );
  };

  window.addEventListener("message", onMessage);
  return {
    dispose() {
      window.removeEventListener("message", onMessage);
      if (windowTimer !== null) clearTimeout(windowTimer);
    },
  };
}
