import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { TreeItemIcon } from "./TreeItemIcon";

describe("TreeItemIcon", () => {
  it("renders a custom emoji", () => {
    const { getByText } = render(
      <TreeItemIcon
        kind="note"
        style={{ icon: { kind: "emoji", value: "📚" } }}
      />,
    );
    expect(getByText("📚")).toBeInTheDocument();
  });

  it("renders a custom lucide icon with its color", () => {
    const { container } = render(
      <TreeItemIcon
        kind="note"
        style={{
          icon: { kind: "lucide", name: "star", color: "rgb(70, 179, 230)" },
        }}
      />,
    );
    const svg = container.querySelector("svg.lucide-star");
    expect(svg).toBeTruthy();
    expect(svg!.getAttribute("stroke")).toBe("rgb(70, 179, 230)");
  });

  it("renders the default filled folder glyph for a folder with no icon", () => {
    const { container } = render(<TreeItemIcon kind="folder" />);
    expect(container.querySelector("svg.lucide-folder")).toBeTruthy();
  });

  it("renders the default outline doc glyph for a note with no icon", () => {
    const { container } = render(<TreeItemIcon kind="note" />);
    expect(container.querySelector("svg.lucide-file-text")).toBeTruthy();
  });
});
