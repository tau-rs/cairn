import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TheirVersionView } from "./TheirVersionView";

describe("TheirVersionView", () => {
  it("shows a read-only diff of their version vs mine", () => {
    render(
      <TheirVersionView
        path="n.md"
        mine={"shared\nmine"}
        theirs={"shared\ntheirs"}
        onBack={() => {}}
        onUseTheirs={() => {}}
      />,
    );
    expect(screen.getByText(/their version/i)).toBeInTheDocument();
    expect(screen.getByText("theirs")).toBeInTheDocument();
    expect(screen.getByText("mine")).toBeInTheDocument();
  });

  it("back and use-their-version are explicit separate actions", async () => {
    const onBack = vi.fn();
    const onUse = vi.fn();
    render(
      <TheirVersionView
        path="n.md"
        mine="a"
        theirs="b"
        onBack={onBack}
        onUseTheirs={onUse}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: /back to my version/i }),
    );
    expect(onBack).toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: /use their version/i }),
    );
    expect(onUse).toHaveBeenCalled();
  });
});
