import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RestoreConfirmDialog } from "./RestoreConfirmDialog";

describe("RestoreConfirmDialog", () => {
  it("names the revision being restored in the title", () => {
    render(
      <RestoreConfirmDialog
        open
        revision="9c2e1"
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );
    expect(screen.getByText(/Restore version 9c2e1/i)).toBeInTheDocument();
    expect(
      screen.getByText(/replaces the note's current contents/i),
    ).toBeInTheDocument();
  });

  it("fires onCancel and onConfirm", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(
      <RestoreConfirmDialog
        open
        revision="9c2e1"
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    fireEvent.click(screen.getByRole("button", { name: /restore/i }));
    expect(onCancel).toHaveBeenCalled();
    expect(onConfirm).toHaveBeenCalled();
  });
});
