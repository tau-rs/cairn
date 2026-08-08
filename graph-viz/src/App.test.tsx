import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("./graph/GraphView", () => ({ default: () => <div data-testid="graph" /> }));
vi.mock("./client/daemonClient", () => ({
  getGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  graphAt: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  graphDiff: vi.fn().mockResolvedValue({ nodesAdded: [], nodesRemoved: [], nodesChanged: [], edgesAdded: [], edgesRemoved: [] }),
  vaultHistory: vi.fn().mockResolvedValue([
    { id: "newest", message: "c3", timestamp_secs: 3, author: "x" },
    { id: "mid", message: "c2", timestamp_secs: 2, author: "x" },
    { id: "oldest", message: "c1", timestamp_secs: 1, author: "x" },
  ]),
  DaemonError: class extends Error {},
}));

import App from "./App";
import * as client from "./client/daemonClient";

describe("App", () => {
  beforeEach(() => vi.clearAllMocks());

  it("loads the live graph on mount", async () => {
    render(<App />);
    await waitFor(() => expect(client.getGraph).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("graph")).toBeInTheDocument();
  });

  it("scrub mode fires graphAt with the selected revision on release", async () => {
    render(<App />);
    await waitFor(() => expect(client.vaultHistory).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /scrub/i }));
    const slider = await screen.findByRole("slider");
    // index 0 = oldest ... last = newest (HEAD). Pick middle revision.
    fireEvent.change(slider, { target: { value: "1" } });
    fireEvent.mouseUp(slider);
    await waitFor(() => expect(client.graphAt).toHaveBeenCalledWith("mid"));
    expect(client.graphAt).toHaveBeenCalledTimes(1);
  });

  it("diff mode fires graphDiff(from,to) ordered oldest→newest on release", async () => {
    render(<App />);
    await waitFor(() => expect(client.vaultHistory).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /diff/i }));
    const [fromSlider, toSlider] = await screen.findAllByRole("slider");
    fireEvent.change(fromSlider, { target: { value: "0" } }); // oldest
    fireEvent.change(toSlider, { target: { value: "2" } }); // newest
    fireEvent.mouseUp(toSlider);
    await waitFor(() => expect(client.graphDiff).toHaveBeenCalledWith("oldest", "newest"));
  });
});
