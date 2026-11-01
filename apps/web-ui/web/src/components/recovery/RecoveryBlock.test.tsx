import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RecoveryBlock } from "./RecoveryBlock";

const item = {
  id: { replica: "3", counter: "88" },
  kind: "deleted" as const,
  versions: ["## Risks\n- lock-in"],
};

describe("RecoveryBlock", () => {
  it("shows kind badge, id, and a diff; Copy fires", () => {
    const onCopy = vi.fn();
    render(
      <RecoveryBlock
        item={item}
        currentText="# Draft"
        layout="unified"
        restoreEnabled
        onCopy={onCopy}
        onRestore={() => {}}
        restoring={false}
      />,
    );
    expect(screen.getByText(/deleted/i)).toBeInTheDocument();
    expect(screen.getByText("#3·88")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: /copy/i })[0]);
    expect(onCopy).toHaveBeenCalledWith("## Risks\n- lock-in");
  });
  it("disables Restore when restoreEnabled is false", () => {
    render(
      <RecoveryBlock
        item={item}
        currentText=""
        layout="unified"
        restoreEnabled={false}
        onCopy={() => {}}
        onRestore={() => {}}
        restoring={false}
      />,
    );
    expect(screen.getByRole("button", { name: /restore/i })).toBeDisabled();
  });
});
