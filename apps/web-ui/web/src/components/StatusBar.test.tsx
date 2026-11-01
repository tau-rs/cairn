import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StatusBar } from "./StatusBar";

const NOW_SECS = Math.floor(Date.now() / 1000);

describe("StatusBar", () => {
  it("shows the three calm axes when healthy", () => {
    render(
      <StatusBar
        saving={false}
        dirty={false}
        sync="ok"
        lastVersion={null}
        onShowVersions={() => {}}
      />,
    );
    expect(screen.getByTestId("status-saved")).toHaveTextContent("Saved");
    expect(screen.getByTestId("status-sync")).toHaveTextContent("Synced");
    expect(
      screen.getByRole("button", { name: /versions/i }),
    ).toBeInTheDocument();
  });

  it("shows Saving… while a flush is in flight", () => {
    render(
      <StatusBar
        saving={true}
        dirty={true}
        sync="ok"
        lastVersion={null}
        onShowVersions={() => {}}
      />,
    );
    expect(screen.getByTestId("status-saved")).toHaveTextContent("Saving…");
  });

  it("is reassuring when offline", () => {
    render(
      <StatusBar
        saving={false}
        dirty={false}
        sync="down"
        lastVersion={null}
        onShowVersions={() => {}}
      />,
    );
    expect(screen.getByTestId("status-sync")).toHaveTextContent(
      "Offline — changes saved locally",
    );
  });

  it("summarizes the last version and opens the browser", async () => {
    const onShow = vi.fn();
    render(
      <StatusBar
        saving={false}
        dirty={false}
        sync="ok"
        lastVersion={{
          id: "c9",
          message: "m",
          author: "a",
          timestamp_secs: NOW_SECS - 60,
          summary: null,
          name: null,
        }}
        onShowVersions={onShow}
      />,
    );
    const lastVersion = screen.getByTestId("status-last-version");
    expect(lastVersion).toHaveTextContent(/Last version:/);
    // No structured or parseable word delta on this fixture → no suffix.
    expect(lastVersion).not.toHaveTextContent(/words/);
    await userEvent.click(screen.getByRole("button", { name: /versions/i }));
    expect(onShow).toHaveBeenCalled();
  });

  it("appends the word-delta suffix when derivable from the last version", () => {
    render(
      <StatusBar
        saving={false}
        dirty={false}
        sync="ok"
        lastVersion={{
          id: "c9",
          message: "m",
          author: "a",
          timestamp_secs: NOW_SECS - 60,
          summary: { files_changed: 1, words_added: 124, words_removed: 3 },
          name: null,
        }}
        onShowVersions={() => {}}
      />,
    );
    expect(screen.getByTestId("status-last-version")).toHaveTextContent(
      /\+124 words/,
    );
  });
});
