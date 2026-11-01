import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { cairnStore } from "../../app/cairnStore";
import { MoreMenu } from "./MoreMenu";

describe("MoreMenu", () => {
  beforeEach(() => {
    cairnStore.getState().setUi({ settingsOpen: false });
  });

  it("opens Settings", async () => {
    render(<MoreMenu />);
    await userEvent.click(screen.getByRole("button", { name: /settings/i }));
    expect(cairnStore.getState().ui.settingsOpen).toBe(true);
  });
});
