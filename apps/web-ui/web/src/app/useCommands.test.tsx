import { describe, it, expect, beforeEach, vi } from "vitest";
import type { ReactNode } from "react";
import { renderHook, act } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { useCommands } from "./useCommands";
import { cairnStore } from "./cairnStore";
import { DEFAULT_UI } from "../store/store";

const wrapper = ({ children }: { children: ReactNode }) => (
  <MemoryRouter>{children}</MemoryRouter>
);

beforeEach(() => {
  cairnStore.setState({ ui: { ...DEFAULT_UI } });
});

describe("useCommands runCommand", () => {
  it("opens the new-note dialog with an empty initial path", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("new-note"));
    expect(cairnStore.getState().ui.newNoteOpen).toBe(true);
    expect(cairnStore.getState().ui.newNoteInitial).toBe("");
  });

  it("opens settings", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("open-settings"));
    expect(cairnStore.getState().ui.settingsOpen).toBe(true);
  });

  it("toggles the command palette", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("open-palette"));
    expect(cairnStore.getState().ui.paletteOpen).toBe(true);
    act(() => result.current.runCommand("open-palette"));
    expect(cairnStore.getState().ui.paletteOpen).toBe(false);
  });

  it("exposes the built-in commands (minus open-palette) plus their hints", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    const ids = result.current.commands.map((c) => c.id);
    expect(ids).not.toContain("open-palette");
  });

  it("split-right command calls splitPane", () => {
    const spy = vi.spyOn(cairnStore.getState(), "splitPane");
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("split-right"));
    expect(spy).toHaveBeenCalled();
  });

  it("close-pane command calls closePane", () => {
    const spy = vi.spyOn(cairnStore.getState(), "closePane");
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("close-pane"));
    expect(spy).toHaveBeenCalled();
  });

  it("open-ask sets ask mode to bar", () => {
    cairnStore.getState().askClose();
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("open-ask"));
    expect(cairnStore.getState().ask.mode).toBe("bar");
    cairnStore.getState().askClose();
  });

  it("exposes recover-lost-work in the command list", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    const ids = result.current.commands.map((c) => c.id);
    expect(ids).toContain("recover-lost-work");
  });

  it("recover-lost-work opens recovery for the active note", () => {
    cairnStore.setState({ activePath: "draft.md" });
    const spy = vi.spyOn(cairnStore.getState(), "openRecovery");
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("recover-lost-work"));
    expect(spy).toHaveBeenCalledWith("draft.md");
  });

  it("recover-lost-work does nothing without an active note", () => {
    cairnStore.setState({ activePath: null });
    const spy = vi.spyOn(cairnStore.getState(), "openRecovery");
    spy.mockClear(); // vi.spyOn reuses an existing spy from a prior test
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("recover-lost-work"));
    expect(spy).not.toHaveBeenCalled();
  });
});
