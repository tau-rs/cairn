import { describe, it, expect } from "vitest";
import {
  toPaletteCommands,
  parsePluginCommandId,
  pluginCommandArgs,
} from "./pluginCommands";
import type { PluginSummary, PluginContribution } from "../../contract";
import type { JsonValue } from "../../contract/serde_json/JsonValue";

const demo: PluginSummary = {
  id: "demo",
  name: "Demo plugin",
  version: "1.0.0",
  commands: [{ id: "stamp", title: "Insert stamp note" }],
  contributions: [],
};

/** Build a `command`-slot `action` contribution fixture (all contract keys). */
function actionContribution(
  command: string,
  label: string,
  args: JsonValue,
): PluginContribution {
  return {
    id: `contrib-${command}`,
    slot: "command",
    widget: { kind: "action", label, icon: null, command, args },
    title: null,
    icon: null,
    order: null,
  };
}

describe("toPaletteCommands", () => {
  it("flattens plugins' commands into palette commands", () => {
    expect(toPaletteCommands([demo])).toEqual([
      { id: "plugin:demo/stamp", label: "Demo plugin: Insert stamp note" },
    ]);
  });
  it("returns [] for no plugins", () => {
    expect(toPaletteCommands([])).toEqual([]);
  });

  it("flattens a command-slot action contribution into a palette command", () => {
    const plugin: PluginSummary = {
      ...demo,
      commands: [],
      contributions: [actionContribution("flash", "Flash widget", null)],
    };
    expect(toPaletteCommands([plugin])).toEqual([
      { id: "plugin:demo/flash", label: "Flash widget" },
    ]);
  });

  it("dedupes legacy + contribution on the same id, contribution label wins", () => {
    const plugin: PluginSummary = {
      ...demo,
      commands: [{ id: "stamp", title: "Insert stamp note" }],
      contributions: [actionContribution("stamp", "Stamp (rich)", null)],
    };
    const out = toPaletteCommands([plugin]);
    expect(out).toEqual([{ id: "plugin:demo/stamp", label: "Stamp (rich)" }]);
  });
});

describe("pluginCommandArgs", () => {
  it("maps a command-slot action's non-null args by palette id", () => {
    const plugin: PluginSummary = {
      ...demo,
      commands: [],
      contributions: [actionContribution("flash", "Flash widget", { n: 2 })],
    };
    expect(pluginCommandArgs([plugin])).toEqual({
      "plugin:demo/flash": { n: 2 },
    });
  });

  it("omits contributions with null args", () => {
    const plugin: PluginSummary = {
      ...demo,
      commands: [],
      contributions: [actionContribution("flash", "Flash widget", null)],
    };
    expect(pluginCommandArgs([plugin])).toEqual({});
  });
});

describe("parsePluginCommandId", () => {
  it("round-trips a plugin command id", () => {
    expect(parsePluginCommandId("plugin:demo/stamp")).toEqual({
      plugin: "demo",
      command: "stamp",
    });
  });
  it("keeps a slash in the command id", () => {
    expect(parsePluginCommandId("plugin:demo/sub/run")).toEqual({
      plugin: "demo",
      command: "sub/run",
    });
  });
  it("returns null for a non-plugin id", () => {
    expect(parsePluginCommandId("new-note")).toBeNull();
    expect(parsePluginCommandId("plugin:demo")).toBeNull();
  });
});
