import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Modal } from "./Modal";

describe("Modal", () => {
  it("renders title + children when open", () => {
    render(
      <Modal open onClose={vi.fn()} title="Hi">
        <div>body</div>
      </Modal>,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("Hi")).toBeInTheDocument();
    expect(screen.getByText("body")).toBeInTheDocument();
  });
  it("renders nothing when closed", () => {
    render(
      <Modal open={false} onClose={vi.fn()} title="Hi">
        <div>body</div>
      </Modal>,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  it("calls onClose on Escape", async () => {
    const onClose = vi.fn();
    render(
      <Modal open onClose={onClose} title="Hi">
        <div>body</div>
      </Modal>,
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});
