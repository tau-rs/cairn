import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PresenceCluster, editingLabel } from "./PresenceCluster";

const base = {
  status: "ok" as const,
  live: false,
  peers: [],
  conflictCount: 0,
  onConflict: () => {},
  onReconnect: () => {},
};

describe("PresenceCluster", () => {
  it("shows a calm Connected chip when healthy and alone", () => {
    render(<PresenceCluster {...base} />);
    expect(screen.getByText("Connected")).toBeInTheDocument();
  });

  it("degrades to 'Someone is editing…' on anonymous live activity", () => {
    render(<PresenceCluster {...base} live={true} />);
    expect(screen.getByText("Someone is editing…")).toBeInTheDocument();
  });

  it("names editors when the roster is present", () => {
    render(
      <PresenceCluster
        {...base}
        live={true}
        peers={[{ id: "1", name: "Maya", editing: true }]}
      />,
    );
    expect(screen.getByText("Maya is editing…")).toBeInTheDocument();
  });

  it("shows N here when peers are present but idle", () => {
    render(<PresenceCluster {...base} peers={[{ id: "1" }, { id: "2" }]} />);
    expect(screen.getByText("2 here")).toBeInTheDocument();
  });

  it("pulses Reconnecting…", () => {
    render(<PresenceCluster {...base} status="reconnecting" />);
    expect(screen.getByText("Reconnecting…")).toBeInTheDocument();
  });

  it("offers Reconnect when offline", async () => {
    const onReconnect = vi.fn();
    render(
      <PresenceCluster {...base} status="down" onReconnect={onReconnect} />,
    );
    expect(screen.getByText("Offline")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Reconnect" }));
    expect(onReconnect).toHaveBeenCalled();
  });

  it("conflict wins over everything and opens the dialog", async () => {
    const onConflict = vi.fn();
    render(
      <PresenceCluster
        {...base}
        status="down"
        live
        conflictCount={2}
        onConflict={onConflict}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: /also changed on another device/i }),
    );
    expect(onConflict).toHaveBeenCalled();
  });
});

describe("editingLabel", () => {
  it("rolls past two names", () => {
    expect(editingLabel([])).toBe("Someone is editing…");
    expect(editingLabel([{ id: "1", name: "Maya", editing: true }])).toBe(
      "Maya is editing…",
    );
    expect(
      editingLabel([
        { id: "1", name: "Maya", editing: true },
        { id: "2", name: "Sam", editing: true },
      ]),
    ).toBe("Maya, Sam editing…");
    expect(
      editingLabel([
        { id: "1", name: "Maya", editing: true },
        { id: "2", name: "Sam", editing: true },
        { id: "3", name: "Ada", editing: true },
      ]),
    ).toBe("Maya, Sam +1 editing…");
  });
});
