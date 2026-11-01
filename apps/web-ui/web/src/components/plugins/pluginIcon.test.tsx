import { describe, it, expect } from "vitest";
import { PLUGIN_ICON_VALUES } from "../../contract";
import { pluginIconNode, PLUGIN_ICON_KEYS } from "./pluginIcon";

describe("pluginIcon registry", () => {
  it("has a non-null node for every contract enum value (lockstep)", () => {
    for (const v of PLUGIN_ICON_VALUES) {
      expect(PLUGIN_ICON_KEYS).toContain(v);
      expect(pluginIconNode(v)).not.toBeNull();
    }
  });

  it("returns null for an unknown icon name", () => {
    expect(pluginIconNode("nope" as never)).toBeNull();
  });
});
