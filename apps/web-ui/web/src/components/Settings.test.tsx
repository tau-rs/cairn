import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Settings } from "./Settings";
import { DEFAULT_SETTINGS } from "../store/store";

describe("Settings", () => {
  it("toggles loading remote images", async () => {
    const onChange = vi.fn();
    render(<Settings settings={DEFAULT_SETTINGS} onChange={onChange} />);
    await userEvent.click(screen.getByLabelText(/load remote images/i));
    expect(onChange).toHaveBeenCalledWith({
      loadRemoteImages: !DEFAULT_SETTINGS.loadRemoteImages,
    });
  });
});
