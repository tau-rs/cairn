import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import type { Breakpoint } from "../responsive/useBreakpoint";

const bp = vi.hoisted(() => ({ value: "desktop" as Breakpoint }));
vi.mock("../responsive/useBreakpoint", () => ({
  useBreakpoint: () => bp.value,
}));

import { AppShell } from "./AppShell";

const regions = {
  topBar: <div>TOPBAR</div>,
  list: <div>LIST</div>,
  editor: <div>EDITOR</div>,
  backlinks: <div>BACKLINKS</div>,
};

function renderAt(tier: Breakpoint) {
  bp.value = tier;
  return render(
    <MemoryRouter>
      <AppShell {...regions} />
    </MemoryRouter>,
  );
}

describe("AppShell", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders the desktop three-pane shell at desktop tier", () => {
    renderAt("desktop");
    expect(screen.getByText("BACKLINKS")).toBeInTheDocument();
    expect(screen.getByText("LIST")).toBeInTheDocument();
  });

  it("renders the bottom nav at mobile tier", () => {
    renderAt("mobile");
    expect(screen.getByRole("navigation")).toBeInTheDocument();
  });
});
