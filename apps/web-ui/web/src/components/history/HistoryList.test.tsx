import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HistoryList } from "./HistoryList";
import type { Revision } from "../../contract/Revision";

const NOW_SECS = Math.floor(Date.now() / 1000);
const noop = () => {};
const revs: Revision[] = [
  {
    id: "c3",
    message: 'Edit "roadmap" (+4/−1 words)',
    author: "a",
    timestamp_secs: NOW_SECS - 60,
    summary: null,
    name: null,
  },
  {
    id: "c2",
    message: "Draft done",
    author: "a",
    timestamp_secs: NOW_SECS - 120,
    summary: null,
    // Named-ness is derived from `name` alone — the engine has no `is_named`.
    name: "Draft 1",
  },
  // > 30 min older: separate session, same day
  {
    id: "c1",
    message: "start",
    author: "a",
    timestamp_secs: NOW_SECS - 4000,
    summary: null,
    name: null,
  },
];

describe("HistoryList (Versions browser)", () => {
  it("groups by day with relative headers and collapses sessions", async () => {
    render(
      <HistoryList
        revisions={revs}
        loading={false}
        onView={noop}
        onRestore={noop}
        onName={noop}
      />,
    );
    expect(screen.getByText("Today")).toBeInTheDocument();
    // c3 and c2 share a session: head visible, rest behind a disclosure
    expect(
      screen.getByText('Edit "roadmap" (+4/−1 words)'),
    ).toBeInTheDocument();
    expect(screen.queryByText("Draft done")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /1 more/i }));
    expect(screen.getByText("Draft done")).toBeInTheDocument();
    // c1 is its own session head, visible without disclosure
    expect(screen.getByText("start")).toBeInTheDocument();
  });

  it("filters to named versions only", async () => {
    render(
      <HistoryList
        revisions={revs}
        loading={false}
        onView={noop}
        onRestore={noop}
        onName={noop}
      />,
    );
    await userEvent.click(screen.getByLabelText(/named only/i));
    expect(screen.getByText("Draft done")).toBeInTheDocument();
    expect(screen.queryByText("start")).not.toBeInTheDocument();
    expect(screen.getByText("Draft 1")).toBeInTheDocument(); // the name badge
  });

  it("offers Name… on a row", async () => {
    const onName = vi.fn();
    render(
      <HistoryList
        revisions={[revs[2]]}
        loading={false}
        onView={noop}
        onRestore={noop}
        onName={onName}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /name…/i }));
    expect(onName).toHaveBeenCalledWith("c1");
  });
});
