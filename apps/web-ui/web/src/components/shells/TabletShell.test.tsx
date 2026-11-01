import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { cairnStore } from "../../app/cairnStore";
import { TabletShell } from "./TabletShell";

const regions = {
  topBar: <div>TOPBAR</div>,
  list: <div>LIST</div>,
  editor: <div>EDITOR</div>,
  backlinks: <div>BACKLINKS</div>,
};

// TabletShell mounts AskSheetHost, which navigates on citation click, so the
// shell must render under a router (as it always does in the app).
const renderShell = () =>
  render(
    <MemoryRouter>
      <TabletShell {...regions} />
    </MemoryRouter>,
  );

describe("TabletShell", () => {
  beforeEach(() => {
    cairnStore.getState().setUi({ backlinksOpen: false });
  });

  it("shows tree + editor and hides backlinks until toggled", () => {
    renderShell();
    expect(screen.getByText("LIST")).toBeInTheDocument();
    expect(screen.getByText("EDITOR")).toBeInTheDocument();
    expect(screen.queryByText("BACKLINKS")).not.toBeInTheDocument();
  });

  it("opens the backlinks drawer via the Links button", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: /links/i }));
    expect(screen.getByText("BACKLINKS")).toBeInTheDocument();
  });
});
