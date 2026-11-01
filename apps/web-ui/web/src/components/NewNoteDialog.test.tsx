import { describe, it, expect, vi } from "vitest";
import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { NewNoteDialog } from "./NewNoteDialog";

describe("NewNoteDialog", () => {
  it("creates from the typed path and closes", async () => {
    const onCreate = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <NewNoteDialog open onOpenChange={onOpenChange} onCreate={onCreate} />,
    );
    await userEvent.type(screen.getByPlaceholderText("notes/idea.md"), "a.md");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(onCreate).toHaveBeenCalledWith("a.md");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
  it("disables Create when empty", () => {
    render(<NewNoteDialog open onOpenChange={vi.fn()} onCreate={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  });
  it("Cancel closes without creating", async () => {
    const onCreate = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <NewNoteDialog open onOpenChange={onOpenChange} onCreate={onCreate} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCreate).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
  it("resets the path field when cancelled and reopened", async () => {
    const onCreate = vi.fn();
    let externalSetOpen: (v: boolean) => void;
    function Wrapper() {
      const [open, setOpen] = useState(true);
      externalSetOpen = setOpen;
      return (
        <NewNoteDialog open={open} onOpenChange={setOpen} onCreate={onCreate} />
      );
    }
    render(<Wrapper />);
    const input = screen.getByPlaceholderText("notes/idea.md");
    await userEvent.type(input, "draft.md");
    expect(input).toHaveValue("draft.md");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCreate).not.toHaveBeenCalled();
    // Reopen — the field re-seeds to its initial value (empty here).
    act(() => externalSetOpen(true));
    expect(screen.getByPlaceholderText("notes/idea.md")).toHaveValue("");
  });
  it("seeds the input from initialPath when opened", () => {
    render(
      <NewNoteDialog
        open={true}
        initialPath="projects/"
        onOpenChange={vi.fn()}
        onCreate={vi.fn()}
      />,
    );
    expect(screen.getByPlaceholderText("notes/idea.md")).toHaveValue(
      "projects/",
    );
  });
});
