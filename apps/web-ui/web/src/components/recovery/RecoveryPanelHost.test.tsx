import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import { RecoveryPanelHost } from "./RecoveryPanelHost";
import { cairnStore } from "../../app/cairnStore";

afterEach(() => {
  cleanup();
  act(() => {
    cairnStore.getState().closeRecovery();
  });
});

describe("RecoveryPanelHost", () => {
  it("renders nothing when recovery is closed", () => {
    render(<RecoveryPanelHost />);
    expect(screen.queryByTestId("recovery-panel")).toBeNull();
  });

  it("shows the panel with fixture blocks once a session opens", async () => {
    render(<RecoveryPanelHost />);

    await act(async () => {
      cairnStore.getState().openRecovery("index.md");
      await Promise.resolve();
    });

    expect(screen.getByTestId("recovery-panel")).toBeInTheDocument();
    expect(screen.getByText("Recovery — index.md")).toBeInTheDocument();
    expect(screen.getByText("Deleted (1)")).toBeInTheDocument();
    expect(screen.getByText("Overwritten (1)")).toBeInTheDocument();
  });
});
