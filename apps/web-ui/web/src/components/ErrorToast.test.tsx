import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorToast } from "./ErrorToast";

describe("ErrorToast", () => {
  it("renders nothing when there are no errors", () => {
    const { container } = render(
      <ErrorToast errors={[]} onDismiss={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows each error and dismisses by id", async () => {
    const onDismiss = vi.fn();
    render(
      <ErrorToast
        errors={[
          { id: 1, message: "boom" },
          { id: 2, message: "kaboom" },
        ]}
        onDismiss={onDismiss}
      />,
    );
    expect(screen.getByText("boom")).toBeInTheDocument();
    expect(screen.getByText("kaboom")).toBeInTheDocument();
    const buttons = screen.getAllByRole("button", { name: /dismiss/i });
    await userEvent.click(buttons[0]);
    expect(onDismiss).toHaveBeenCalledWith(1);
  });
});
