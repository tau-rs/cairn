import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { Shell } from "./Shell";
import { cairnStore } from "../app/cairnStore";

afterEach(() => {
  cleanup();
  cairnStore.setState({ pluginContributions: {} } as never);
});

const regions = {
  topBar: <div>top</div>,
  list: <div>list</div>,
  editor: <div>editor</div>,
  backlinks: <div>backlinks</div>,
};

describe("Shell", () => {
  it("renders the three regions and top bar", () => {
    render(<Shell {...regions} />);
    expect(screen.getByText("top")).toBeInTheDocument();
    expect(screen.getByText("list")).toBeInTheDocument();
    expect(screen.getByText("editor")).toBeInTheDocument();
    expect(screen.getByText("backlinks")).toBeInTheDocument();
    // No panel.main contributions → the plugin dock stays unmounted.
    expect(screen.queryByLabelText("Plugin panel")).toBeNull();
  });

  it("mounts the plugin dock when a panel.main contribution is present", () => {
    cairnStore.setState({
      pluginContributions: {
        "panel.main": [
          {
            plugin: "p",
            epoch: 1,
            c: {
              id: "d",
              slot: "panel.main",
              title: "Dock",
              icon: null,
              order: null,
              widget: { kind: "text", text: "docked", muted: null },
            },
          },
        ],
      },
    } as never);
    render(<Shell {...regions} />);
    expect(screen.getByLabelText("Plugin panel")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Dock" })).toBeInTheDocument();
  });
});
