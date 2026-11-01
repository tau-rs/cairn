import { describe, expect, it } from "vitest";
import {
  groupBySlot,
  sanitizeContributions,
  sanitizeCapabilities,
  type SanitizeReport,
  type SlotEntry,
  PLUGIN_SLOTS,
  WIDGET_KINDS,
  MAX_CONTRIBS_PER_PLUGIN,
  MAX_LIST_ITEMS,
  MAX_STR,
} from "./pluginContributions";
import {
  PLUGIN_SLOT_VALUES,
  PLUGIN_WIDGET_KIND_VALUES,
  type PluginContribution,
  type PluginSummary,
} from "../contract";
import {
  MAX_IFRAME_HEIGHT,
  MIN_IFRAME_HEIGHT,
  DEFAULT_IFRAME_HEIGHT,
} from "./pluginTier3";

function freshReport(): SanitizeReport {
  return { kept: 0, dropped: 0, reasons: [] };
}

describe("sanitizeContributions", () => {
  it("returns [] when input is not an array", () => {
    expect(sanitizeContributions(null)).toEqual([]);
    expect(sanitizeContributions({})).toEqual([]);
    expect(sanitizeContributions("nope")).toEqual([]);
  });

  it("keeps a well-formed text contribution with all null keys present", () => {
    const report = freshReport();
    const out = sanitizeContributions(
      [
        {
          id: "c1",
          slot: "sidebar.section",
          widget: { kind: "text", text: "hello" },
          title: "Title",
        },
      ],
      report,
    );
    expect(out).toEqual<PluginContribution[]>([
      {
        id: "c1",
        slot: "sidebar.section",
        widget: { kind: "text", text: "hello", muted: null },
        title: "Title",
        icon: null,
        order: null,
      },
    ]);
    expect(report.kept).toBe(1);
    expect(report.dropped).toBe(0);
  });

  it("emits a fully-populated action widget", () => {
    const out = sanitizeContributions([
      {
        id: "a1",
        slot: "topbar.action",
        widget: { kind: "action", label: "Go", command: "do.it" },
      },
    ]);
    expect(out[0].widget).toEqual({
      kind: "action",
      label: "Go",
      icon: null,
      command: "do.it",
      args: null,
    });
  });

  it("emits a fully-populated list widget", () => {
    const out = sanitizeContributions([
      {
        id: "l1",
        slot: "sidebar.section",
        widget: { kind: "list", items: [{ id: "i1", label: "Row" }] },
      },
    ]);
    expect(out[0].widget).toEqual({
      kind: "list",
      items: [
        { id: "i1", label: "Row", icon: null, command: null, args: null },
      ],
    });
  });

  it("drops an unknown slot", () => {
    const report = freshReport();
    const out = sanitizeContributions(
      [{ id: "x", slot: "bogus", widget: { kind: "text", text: "t" } }],
      report,
    );
    expect(out).toEqual([]);
    expect(report.dropped).toBe(1);
    expect(report.reasons.length).toBeGreaterThan(0);
  });

  it("drops an unknown widget kind", () => {
    const out = sanitizeContributions([
      { id: "x", slot: "command", widget: { kind: "iframe" } },
    ]);
    expect(out).toEqual([]);
  });

  it("drops a contribution missing a string id", () => {
    const out = sanitizeContributions([
      { slot: "sidebar.section", widget: { kind: "text", text: "t" } },
      { id: 7, slot: "sidebar.section", widget: { kind: "text", text: "t" } },
    ]);
    expect(out).toEqual([]);
  });

  it("drops a command-slot contribution whose widget is not action", () => {
    const out = sanitizeContributions([
      { id: "c", slot: "command", widget: { kind: "text", text: "t" } },
    ]);
    expect(out).toEqual([]);
  });

  it("keeps a command-slot contribution whose widget is action", () => {
    const out = sanitizeContributions([
      {
        id: "c",
        slot: "command",
        widget: { kind: "action", label: "Run", command: "run" },
      },
    ]);
    expect(out).toHaveLength(1);
    expect(out[0].slot).toBe("command");
  });

  it("clamps over-long text to MAX_STR", () => {
    const big = "x".repeat(10000);
    const out = sanitizeContributions([
      { id: "c", slot: "sidebar.section", widget: { kind: "text", text: big } },
    ]);
    const w = out[0].widget;
    if (w.kind !== "text") throw new Error("expected text");
    expect(w.text).toHaveLength(MAX_STR);
  });

  it("clamps over-long title and label to MAX_STR", () => {
    const big = "y".repeat(9000);
    const out = sanitizeContributions([
      {
        id: "c",
        slot: "topbar.action",
        title: big,
        widget: { kind: "action", label: big, command: "cmd" },
      },
    ]);
    expect(out[0].title).toHaveLength(MAX_STR);
    const w = out[0].widget;
    if (w.kind !== "action") throw new Error("expected action");
    expect(w.label).toHaveLength(MAX_STR);
  });

  it("truncates a 10000-item list to MAX_LIST_ITEMS", () => {
    const items = Array.from({ length: 10000 }, (_, i) => ({
      id: `i${i}`,
      label: `row ${i}`,
    }));
    const out = sanitizeContributions([
      { id: "c", slot: "sidebar.section", widget: { kind: "list", items } },
    ]);
    const w = out[0].widget;
    if (w.kind !== "list") throw new Error("expected list");
    expect(w.items).toHaveLength(MAX_LIST_ITEMS);
  });

  it("drops a contribution whose args exceeds MAX_ARGS_BYTES", () => {
    const report = freshReport();
    const huge = { blob: "z".repeat(20000) };
    const out = sanitizeContributions(
      [
        {
          id: "c",
          slot: "topbar.action",
          widget: { kind: "action", label: "L", command: "cmd", args: huge },
        },
      ],
      report,
    );
    expect(out).toEqual([]);
    expect(report.dropped).toBe(1);
  });

  it("caps a 1000-contribution array to MAX_CONTRIBS_PER_PLUGIN", () => {
    const raw = Array.from({ length: 1000 }, (_, i) => ({
      id: `c${i}`,
      slot: "sidebar.section",
      widget: { kind: "text", text: "t" },
    }));
    const out = sanitizeContributions(raw);
    expect(out).toHaveLength(MAX_CONTRIBS_PER_PLUGIN);
  });

  it("coerces an out-of-enum icon to null", () => {
    const out = sanitizeContributions([
      {
        id: "c",
        slot: "sidebar.section",
        icon: "skull",
        widget: { kind: "text", text: "t" },
      },
    ]);
    expect(out[0].icon).toBeNull();
  });

  it("keeps a valid in-enum icon", () => {
    const out = sanitizeContributions([
      {
        id: "c",
        slot: "sidebar.section",
        icon: "star",
        widget: { kind: "text", text: "t" },
      },
    ]);
    expect(out[0].icon).toBe("star");
  });

  it("fills the SanitizeReport on drops", () => {
    const report = freshReport();
    sanitizeContributions(
      [
        {
          id: "ok",
          slot: "sidebar.section",
          widget: { kind: "text", text: "t" },
        },
        { id: "bad", slot: "nope", widget: { kind: "text", text: "t" } },
      ],
      report,
    );
    expect(report.kept).toBe(1);
    expect(report.dropped).toBeGreaterThan(0);
    expect(report.reasons.length).toBeGreaterThan(0);
  });
});

describe("allow-list lockstep with contract", () => {
  it("PLUGIN_SLOTS is a superset of PLUGIN_SLOT_VALUES", () => {
    for (const v of PLUGIN_SLOT_VALUES) {
      expect(PLUGIN_SLOTS).toContain(v);
    }
  });
  it("WIDGET_KINDS is a superset of PLUGIN_WIDGET_KIND_VALUES", () => {
    for (const v of PLUGIN_WIDGET_KIND_VALUES) {
      expect(WIDGET_KINDS).toContain(v);
    }
  });
});

describe("groupBySlot", () => {
  function plugin(id: string, contributions: unknown[]): PluginSummary {
    return {
      id,
      name: id,
      version: "1.0.0",
      commands: [],
      contributions: contributions as PluginContribution[],
    };
  }

  it("groups by slot and sorts by (order, plugin, id)", () => {
    const plugins = [
      plugin("zeta", [
        {
          id: "b",
          slot: "sidebar.section",
          order: 5,
          widget: { kind: "text", text: "t" },
        },
        {
          id: "a",
          slot: "sidebar.section",
          order: null,
          widget: { kind: "text", text: "t" },
        },
      ]),
      plugin("alpha", [
        {
          id: "a",
          slot: "sidebar.section",
          order: 5,
          widget: { kind: "text", text: "t" },
        },
        {
          id: "topbar",
          slot: "topbar.action",
          widget: { kind: "action", label: "L", command: "cmd" },
        },
      ]),
    ];
    const report = freshReport();
    const grouped = groupBySlot(plugins, 3, report);

    expect(Object.keys(grouped).sort()).toEqual([
      "sidebar.section",
      "topbar.action",
    ]);

    const sidebar = grouped["sidebar.section"];
    // order 5 first (alpha/a before zeta/b by plugin), then null-order zeta/a last.
    expect(sidebar.map((e: SlotEntry) => `${e.plugin}/${e.c.id}`)).toEqual([
      "alpha/a",
      "zeta/b",
      "zeta/a",
    ]);
    expect(sidebar[0].epoch).toBe(3);

    expect(grouped["topbar.action"]).toHaveLength(1);
    expect(report.kept).toBe(4);
  });
});

describe("Tier-3 iframe widget sanitization", () => {
  it("accepts an iframe widget and clamps height into range", () => {
    const out = sanitizeContributions([
      {
        id: "wc",
        slot: "sidebar.section",
        widget: { kind: "iframe", entry: "index.html", height: 9999 },
      },
    ]);
    expect(out).toHaveLength(1);
    const w = out[0].widget as unknown as {
      kind: string;
      entry: string;
      height: number | null;
    };
    expect(w.kind).toBe("iframe");
    expect(w.entry).toBe("index.html");
    expect(w.height).toBe(MAX_IFRAME_HEIGHT);
  });

  it("drops an iframe widget whose entry is unsafe", () => {
    const report = { kept: 0, dropped: 0, reasons: [] as string[] };
    const out = sanitizeContributions(
      [
        {
          id: "esc",
          slot: "sidebar.section",
          widget: { kind: "iframe", entry: "../x.html", height: null },
        },
        {
          id: "abs",
          slot: "sidebar.section",
          widget: { kind: "iframe", entry: "/abs", height: null },
        },
      ],
      report,
    );
    expect(out).toHaveLength(0);
    expect(report.reasons.join()).toMatch(/unsafe or missing entry/);
  });

  it("accepts an iframe entry in the panel.main slot", () => {
    const out = sanitizeContributions([
      {
        id: "panel",
        slot: "panel.main",
        widget: { kind: "iframe", entry: "index.html", height: null },
      },
    ]);
    expect(out).toHaveLength(1);
    expect(out[0].slot).toBe("panel.main");
  });

  it("rejects an iframe widget outside sidebar.section and panel.main", () => {
    // topbar.action exercises the dedicated iframe-slot guard (command would be
    // caught earlier by the command-requires-action guard instead).
    const out = sanitizeContributions([
      {
        id: "x",
        slot: "topbar.action",
        widget: { kind: "iframe", entry: "index.html", height: null },
      },
    ]);
    expect(out).toHaveLength(0);
  });

  it("clamps low height up, passes null through, and defaults a non-number", () => {
    const heightOf = (h: unknown): number | null => {
      const out = sanitizeContributions([
        {
          id: "h",
          slot: "sidebar.section",
          widget: { kind: "iframe", entry: "index.html", height: h },
        },
      ]);
      return (out[0].widget as unknown as { height: number | null }).height;
    };
    expect(heightOf(1)).toBe(MIN_IFRAME_HEIGHT); // below floor → clamped up
    expect(heightOf(null)).toBeNull(); // explicit null preserved
    expect(heightOf("tall")).toBe(DEFAULT_IFRAME_HEIGHT); // non-number → default
  });

  it("sanitizeCapabilities keeps known values and drops unknown ones", () => {
    expect(
      sanitizeCapabilities(["activeNote.write", "filesystem.format", 7]),
    ).toEqual(["activeNote.write"]);
    expect(sanitizeCapabilities(null)).toEqual([]);
    expect(sanitizeCapabilities("nope")).toEqual([]);
  });
});

describe("sanitizeContributions — sanitizer edges (mutation-hardening)", () => {
  const iframe = (entry: unknown) => [
    {
      id: "f",
      slot: "sidebar.section",
      widget: { kind: "iframe", entry, height: null },
    },
  ];

  it("drops an iframe entry containing a backslash (Windows-style traversal)", () => {
    // Guards `x.includes("\\")` in safeEntry — a backslash must be rejected
    // even though it is not a POSIX separator.
    expect(sanitizeContributions(iframe("sub\\evil.html"))).toHaveLength(0);
  });

  it("drops an iframe entry with an empty segment (a//b)", () => {
    // Guards the `seg === ""` arm of safeEntry's segment scan.
    expect(sanitizeContributions(iframe("a//b.html"))).toHaveLength(0);
  });

  it("drops an empty iframe entry and one longer than MAX_ENTRY", () => {
    expect(sanitizeContributions(iframe(""))).toHaveLength(0);
    expect(
      sanitizeContributions(iframe("a".repeat(513) + ".html")),
    ).toHaveLength(0);
  });

  it("keeps an iframe entry of exactly the max length", () => {
    // Boundary: length === MAX_ENTRY (512) must pass (`> MAX_ENTRY`, not `>=`).
    const entry = "a".repeat(512);
    expect(sanitizeContributions(iframe(entry))).toHaveLength(1);
  });

  it("drops a contribution that is an array, not an object", () => {
    // Guards `!Array.isArray(x)` in isRecord — an array must not slip through
    // as a record.
    expect(sanitizeContributions([["not", "an", "object"]])).toEqual([]);
  });

  it("drops a widget that is an array", () => {
    expect(
      sanitizeContributions([{ id: "w", slot: "sidebar.section", widget: [] }]),
    ).toEqual([]);
  });

  it("keeps text of exactly MAX_STR but truncates MAX_STR+1", () => {
    const exact = "x".repeat(MAX_STR);
    const over = "y".repeat(MAX_STR + 1);
    const [a] = sanitizeContributions([
      {
        id: "a",
        slot: "sidebar.section",
        widget: { kind: "text", text: exact },
      },
    ]);
    const [b] = sanitizeContributions([
      {
        id: "b",
        slot: "sidebar.section",
        widget: { kind: "text", text: over },
      },
    ]);
    expect((a.widget as { text: string }).text).toHaveLength(MAX_STR);
    expect((b.widget as { text: string }).text).toHaveLength(MAX_STR);
  });

  it("coerces a non-finite `order` to null but keeps a finite one", () => {
    const [nan] = sanitizeContributions([
      {
        id: "n",
        slot: "sidebar.section",
        widget: { kind: "text", text: "t" },
        order: Number.NaN,
      },
    ]);
    const [ok] = sanitizeContributions([
      {
        id: "o",
        slot: "sidebar.section",
        widget: { kind: "text", text: "t" },
        order: 3,
      },
    ]);
    expect(nan.order).toBeNull();
    expect(ok.order).toBe(3);
  });

  it("passes an action widget with absent args through as null", () => {
    // Guards `x === undefined || x === null` in checkArgs.
    const [c] = sanitizeContributions([
      {
        id: "a",
        slot: "command",
        widget: { kind: "action", command: "do.it" },
      },
    ]);
    expect((c.widget as { args: unknown }).args).toBeNull();
  });

  it("breaks a sort tie on c.id when order and plugin are equal", () => {
    // Guards the final `a.c.id.localeCompare(b.c.id)` tie-break in groupBySlot.
    const mk = (id: string): PluginSummary => ({
      id: "same-plugin",
      name: "P",
      version: "1.0.0",
      commands: [],
      contributions: [
        {
          id,
          slot: "sidebar.section",
          widget: { kind: "text", text: id, muted: null },
          title: null,
          icon: null,
          order: 1,
        },
      ],
    });
    const grouped = groupBySlot([mk("zeta"), mk("alpha")], 0);
    expect(grouped["sidebar.section"].map((e) => e.c.id)).toEqual([
      "alpha",
      "zeta",
    ]);
  });
});
