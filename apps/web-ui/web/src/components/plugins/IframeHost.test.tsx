import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { IframeHost } from "./IframeHost";
import type { BrokerHost } from "../../client/pluginBrokerHost";

const host: BrokerHost = {
  info: () => ({ appVersion: "1", theme: "dark", activePath: null }),
  notice: vi.fn(),
  activeNote: () => null,
  writeActiveNote: vi.fn(),
  readNote: async () => null,
  search: async () => [],
  invokeOwnCommand: async () => {},
  subscribeActiveNote: () => () => {},
};

describe("IframeHost", () => {
  it("renders a sandboxed iframe pointed at the plugin-sandbox origin", () => {
    render(
      <IframeHost
        plugin="p"
        entry="index.html"
        height={200}
        granted={new Set()}
        pluginCommands={new Set()}
        host={host}
      />,
    );
    const frame = screen.getByTitle("plugin:p") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe(
      "allow-scripts allow-same-origin",
    );
    expect(frame.getAttribute("src")).toBe("plugin-sandbox://p/index.html");
    expect(frame.style.height).toBe("200px");
  });

  it("shows the WidgetError fallback if the handshake times out", async () => {
    render(
      <IframeHost
        plugin="p"
        entry="index.html"
        height={null}
        granted={new Set()}
        pluginCommands={new Set()}
        host={host}
        handshakeTimeoutMs={10}
      />,
    );
    await waitFor(() =>
      expect(screen.getByText(/didn't start/i)).toBeInTheDocument(),
    );
  });

  it("forwards activeNote events to the frame when read is granted", () => {
    let fire: (() => void) | null = null;
    const noteHost: BrokerHost = {
      ...host,
      activeNote: () => ({ path: "a.md", title: "a", text: "hi" }),
      subscribeActiveNote: (cb) => {
        fire = cb;
        return () => {
          fire = null;
        };
      },
    };
    const { container } = render(
      <IframeHost
        plugin="p"
        entry="index.html"
        height={null}
        granted={new Set(["activeNote.read"])}
        pluginCommands={new Set()}
        host={noteHost}
      />,
    );
    const frame = container.querySelector("iframe") as HTMLIFrameElement;
    const post = vi.spyOn(frame.contentWindow as Window, "postMessage");
    if (!fire) throw new Error("expected a subscription with read grant");
    (fire as () => void)();
    expect(post).toHaveBeenCalledWith(
      {
        t: "event",
        topic: "activeNote",
        payload: { path: "a.md", title: "a", text: "hi" },
      },
      "*",
    );
  });

  it("does not subscribe to active-note changes without the read grant", () => {
    let subscribed = false;
    const noteHost: BrokerHost = {
      ...host,
      subscribeActiveNote: () => {
        subscribed = true;
        return () => {};
      },
    };
    render(
      <IframeHost
        plugin="p"
        entry="index.html"
        height={null}
        granted={new Set()}
        pluginCommands={new Set()}
        host={noteHost}
      />,
    );
    expect(subscribed).toBe(false);
  });

  it("re-arms the handshake after retry (effect re-runs, not stuck)", async () => {
    render(
      <IframeHost
        plugin="p"
        entry="index.html"
        height={null}
        granted={new Set()}
        pluginCommands={new Set()}
        host={host}
        handshakeTimeoutMs={10}
      />,
    );
    await waitFor(() =>
      expect(screen.getByText(/didn't start/i)).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: /didn't start/i }));
    // Iframe is shown again while re-handshaking...
    expect(screen.getByTitle("plugin:p")).toBeInTheDocument();
    // ...and the timer re-arms, returning to the error state (proves the effect
    // re-ran rather than leaving the frame dead).
    await waitFor(() =>
      expect(screen.getByText(/didn't start/i)).toBeInTheDocument(),
    );
  });
});
