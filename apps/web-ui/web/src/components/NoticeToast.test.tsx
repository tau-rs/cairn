import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { NoticeToast } from "./NoticeToast";

describe("NoticeToast", () => {
  it("renders nothing when message is null", () => {
    const { container } = render(
      <NoticeToast message={null} onDismiss={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });
  it("renders the message and dismisses", () => {
    const onDismiss = vi.fn();
    render(<NoticeToast message="stamp.md" onDismiss={onDismiss} />);
    expect(screen.getByText("stamp.md")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /dismiss notice/i }));
    expect(onDismiss).toHaveBeenCalled();
  });
});
