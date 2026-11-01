import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { cairnStore } from "../../app/cairnStore";
import { PluginsPanel } from "./PluginsPanel";

const sample = [
  {
    id: "demo",
    name: "Demo plugin",
    version: "1.0.0",
    commands: [{ id: "stamp", title: "Insert stamp note" }],
    contributions: [],
  },
];

describe("PluginsPanel", () => {
  it("lists each plugin's name/version and command titles", () => {
    render(
      <PluginsPanel
        plugins={[
          {
            id: "demo",
            name: "Demo plugin",
            version: "1.0.0",
            commands: [{ id: "stamp", title: "Insert stamp note" }],
            contributions: [],
          },
        ]}
      />,
    );
    expect(screen.getByText(/Demo plugin/)).toBeInTheDocument();
    expect(screen.getByText(/v1\.0\.0/)).toBeInTheDocument();
    expect(screen.getByText("Insert stamp note")).toBeInTheDocument();
  });
  it("shows an empty state when there are no plugins", () => {
    render(<PluginsPanel plugins={[]} />);
    expect(screen.getByText(/no plugins loaded/i)).toBeInTheDocument();
  });
  it("surfaces a not-rendered line when contributions were dropped", () => {
    render(<PluginsPanel plugins={sample} dropped={3} />);
    expect(screen.getByText(/3 .*not rendered/i)).toBeInTheDocument();
  });
  it("renders no not-rendered line when dropped is 0", () => {
    render(<PluginsPanel plugins={sample} dropped={0} />);
    expect(screen.queryByText(/not rendered/i)).toBeNull();
  });
});

describe("PluginsPanel grants", () => {
  it("shows a Revoke action for a granted plugin and revokes it", () => {
    cairnStore.setState({
      pluginGrants: { p: { version: "1", granted: ["activeNote.write"] } },
    } as never);
    render(
      <PluginsPanel
        plugins={[
          {
            id: "p",
            name: "Word Linter",
            version: "1",
            commands: [],
            contributions: [],
            capabilities: ["activeNote.write"],
          } as never,
        ]}
      />,
    );
    const btn = screen.getByRole("button", { name: /revoke/i });
    fireEvent.click(btn);
    expect(cairnStore.getState().pluginGrants.p).toBeUndefined();
  });

  it("shows no Revoke for a plugin without granted permissions", () => {
    cairnStore.setState({ pluginGrants: {} } as never);
    render(
      <PluginsPanel
        plugins={[
          {
            id: "q",
            name: "Static",
            version: "1",
            commands: [],
            contributions: [],
            capabilities: null,
          } as never,
        ]}
      />,
    );
    expect(screen.queryByRole("button", { name: /revoke/i })).toBeNull();
  });
});
