import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { RecoverySheet } from "./RecoverySheet";
import type { WireRecoverableBlock } from "../../contract";

const tombstoned: WireRecoverableBlock = {
  id: { replica: "1", counter: "1" },
  tombstoned: true,
  versions: ["## Risks\n- lock-in"],
};

const live: WireRecoverableBlock = {
  id: { replica: "2", counter: "5" },
  tombstoned: false,
  versions: ["old text"],
};

function baseProps() {
  return {
    open: true,
    side: "bottom" as const,
    note: "Plan",
    status: "ready" as const,
    blocks: [tombstoned, live] as WireRecoverableBlock[],
    error: null,
    restoring: null,
    currentText: "current",
    restoreEnabled: true,
    onCopy: vi.fn(),
    onRestore: vi.fn(),
    onClose: vi.fn(),
  };
}

describe("RecoverySheet", () => {
  it("renders the recovery panel's groups inside the drawer when open", () => {
    render(<RecoverySheet {...baseProps()} />);
    expect(screen.getByTestId("recovery-sheet")).toBeInTheDocument();
    expect(screen.getByText("Deleted (1)")).toBeInTheDocument();
    expect(screen.getByText("Overwritten (1)")).toBeInTheDocument();
  });

  it("renders nothing when closed", () => {
    render(<RecoverySheet {...baseProps()} open={false} />);
    expect(screen.queryByTestId("recovery-sheet")).toBeNull();
    expect(screen.queryByText("Deleted (1)")).toBeNull();
  });

  it("renders as a right side sheet too", () => {
    render(<RecoverySheet {...baseProps()} side="right" />);
    expect(screen.getByTestId("recovery-sheet")).toBeInTheDocument();
  });
});
