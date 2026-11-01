import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { CommandPalette } from "./CommandPalette";

const commands = [
  { id: "new-note", label: "New note" },
  { id: "commit", label: "Commit changes" },
];
const notes = ["index.md", "ideas.md"];

function setup(
  over: Partial<React.ComponentProps<typeof CommandPalette>> = {},
) {
  const props = {
    open: true,
    onClose: vi.fn(),
    commands,
    notes,
    onRunCommand: vi.fn(),
    onOpenNote: vi.fn(),
    ...over,
  };
  render(<CommandPalette {...props} />);
  return props;
}

describe("CommandPalette", () => {
  it("shows commands and notes when open", () => {
    setup();
    expect(screen.getByText("New note")).toBeInTheDocument();
    expect(screen.getByText("Commit changes")).toBeInTheDocument();
    expect(screen.getByText("ideas")).toBeInTheDocument(); // stem label
  });
  it("filters by the query and runs the matching command on Enter", () => {
    const props = setup();
    const input = screen.getByPlaceholderText(/type a command/i);
    fireEvent.change(input, { target: { value: "commit" } });
    expect(screen.queryByText("New note")).not.toBeInTheDocument();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onRunCommand).toHaveBeenCalledWith("commit");
    expect(props.onClose).toHaveBeenCalled();
  });
  it("opens a note on Enter when a note is highlighted", () => {
    const props = setup();
    const input = screen.getByPlaceholderText(/type a command/i);
    fireEvent.change(input, { target: { value: "ideas" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onOpenNote).toHaveBeenCalledWith("ideas.md");
  });
  it("ArrowDown moves the highlight before running", () => {
    const props = setup();
    const input = screen.getByPlaceholderText(/type a command/i);
    fireEvent.keyDown(input, { key: "ArrowDown" }); // 1st -> 2nd item
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onRunCommand).toHaveBeenCalledWith("commit");
  });
  it("renders a command's shortcut hint", () => {
    render(
      <CommandPalette
        open
        onClose={vi.fn()}
        commands={[{ id: "commit", label: "Commit changes", hint: "⌘↵" }]}
        notes={[]}
        onRunCommand={vi.fn()}
        onOpenNote={vi.fn()}
      />,
    );
    expect(screen.getByText("⌘↵")).toBeInTheDocument();
  });
});
