import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Editor } from "./Editor";

const base = {
  path: "a.md" as string | null,
  value: "# Hi\n\nlink [[ideas]]",
  notePaths: ["ideas.md"],
  mode: "livepreview" as "livepreview" | "source",
  assetUrl: (p: string) => p,
  loadRemoteImages: false,
  onChange: vi.fn(),
  onOpenNote: vi.fn(),
  onToggleMode: vi.fn(),
};

describe("Editor", () => {
  it("shows a placeholder when no note is open", () => {
    render(<Editor {...base} path={null} />);
    expect(screen.getByText(/no note open/i)).toBeInTheDocument();
  });
  it("live preview mode mounts a CodeMirror editor", () => {
    const { container } = render(<Editor {...base} mode="livepreview" />);
    expect(container.querySelector(".cm-editor")).not.toBeNull();
  });
  it("source mode mounts a CodeMirror editor", () => {
    const { container } = render(<Editor {...base} mode="source" />);
    expect(container.querySelector(".cm-editor")).not.toBeNull();
  });
  it("the toggle button flips the mode", async () => {
    const onToggleMode = vi.fn();
    render(<Editor {...base} mode="livepreview" onToggleMode={onToggleMode} />);
    await userEvent.click(screen.getByRole("button", { name: /source/i }));
    expect(onToggleMode).toHaveBeenCalled();
  });
});
