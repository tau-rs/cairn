import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WidgetView } from "./WidgetView";
import { cairnStore } from "../../app/cairnStore";

describe("WidgetView iframe branch", () => {
  it("shows the permission prompt when the plugin has no grant", () => {
    cairnStore.setState({
      plugins: [
        {
          id: "p",
          name: "Word Linter",
          version: "1",
          commands: [],
          contributions: [],
          capabilities: ["activeNote.write"],
        },
      ],
      pluginGrants: {},
    } as never);
    render(
      <WidgetView
        plugin="p"
        widget={{ kind: "iframe", entry: "index.html", height: 200 } as never}
      />,
    );
    expect(screen.getByText(/wants to:/i)).toBeInTheDocument();
    expect(screen.getByText(/Modify the current note/i)).toBeInTheDocument();
  });

  it("mounts the iframe once the plugin is granted", () => {
    cairnStore.setState({
      plugins: [
        {
          id: "p",
          name: "Word Linter",
          version: "1",
          commands: [],
          contributions: [],
          capabilities: ["activeNote.write"],
        },
      ],
      pluginGrants: { p: { version: "1", granted: ["activeNote.write"] } },
    } as never);
    render(
      <WidgetView
        plugin="p"
        widget={{ kind: "iframe", entry: "index.html", height: 200 } as never}
      />,
    );
    expect(screen.getByTitle("plugin:p")).toBeInTheDocument();
  });

  it("re-prompts when the plugin version bumps past the granted version", () => {
    cairnStore.setState({
      plugins: [
        {
          id: "p",
          name: "Word Linter",
          version: "2", // upgraded
          commands: [],
          contributions: [],
          capabilities: ["activeNote.write"],
        },
      ],
      pluginGrants: { p: { version: "1", granted: ["activeNote.write"] } }, // stale grant
    } as never);
    render(
      <WidgetView
        plugin="p"
        widget={{ kind: "iframe", entry: "index.html", height: 200 } as never}
      />,
    );
    // grant is for v1 but plugin is v2 → consent required again, no iframe yet.
    expect(screen.getByText(/wants to:/i)).toBeInTheDocument();
    expect(screen.queryByTitle("plugin:p")).toBeNull();
  });
});
