import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup, act } from "@testing-library/react";
import { CollabPresenceHost } from "./CollabPresenceHost";
import { cairnStore } from "../../app/cairnStore";

afterEach(() => {
  cleanup();
  act(() => {
    cairnStore.getState().collabStop();
  });
});

describe("CollabPresenceHost", () => {
  it("renders nothing (presence now lives in the TopBar PresenceCluster)", () => {
    const { container } = render(<CollabPresenceHost />);
    expect(container).toBeEmptyDOMElement();
  });

  it("follows the active note on mount", () => {
    act(() => {
      cairnStore.setState({ activePath: "n.md" });
    });

    render(<CollabPresenceHost />);

    expect(cairnStore.getState().collab.note).toBe("n.md");
  });

  it("stops following when unmounted", () => {
    act(() => {
      cairnStore.setState({ activePath: "n.md" });
    });

    const { unmount } = render(<CollabPresenceHost />);
    expect(cairnStore.getState().collab.note).toBe("n.md");

    unmount();
    expect(cairnStore.getState().collab.note).toBeNull();
  });
});
