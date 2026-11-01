import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PermissionPrompt } from "./PermissionPrompt";

describe("PermissionPrompt", () => {
  it("lists grouped risks (HIGH first) and hides silent capabilities", () => {
    render(
      <PermissionPrompt
        name="Word Linter"
        capabilities={[
          "command.invoke",
          "activeNote.read",
          "activeNote.write",
          "notes.read",
        ]}
        onAllow={() => {}}
        onDeny={() => {}}
      />,
    );
    const rows = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(rows[0]).toMatch(/Modify the current note/); // high first
    expect(rows.join()).toMatch(/Read across your whole vault/);
    expect(rows.join()).not.toMatch(/command\.invoke/); // silent
  });

  it("shows a no-special-access message when there are no risk rows", () => {
    render(
      <PermissionPrompt
        name="Static"
        capabilities={["command.invoke"]}
        onAllow={() => {}}
        onDeny={() => {}}
      />,
    );
    expect(screen.getByText(/no special access/i)).toBeInTheDocument();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("fires onAllow / onDeny", () => {
    const onAllow = vi.fn();
    const onDeny = vi.fn();
    render(
      <PermissionPrompt
        name="P"
        capabilities={["activeNote.write"]}
        onAllow={onAllow}
        onDeny={onDeny}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /allow/i }));
    fireEvent.click(screen.getByRole("button", { name: /don't run/i }));
    expect(onAllow).toHaveBeenCalled();
    expect(onDeny).toHaveBeenCalled();
  });
});
