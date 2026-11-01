import { describe, it, expect, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import { useGlobalKeys } from "./useGlobalKeys";
import { eventToChord } from "./keybinding";
import { cairnStore } from "../../app/cairnStore";

describe("useGlobalKeys", () => {
  it("dispatches a mapped chord to runCommand", () => {
    const run = vi.fn();
    // Derive the canonical chord from the same event shape we dispatch, so the
    // test is independent of the platform's Mod-key mapping.
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "open-palette" }, run));
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    );
    expect(run).toHaveBeenCalledWith("open-palette");
  });

  it("ignores an unmapped chord", () => {
    const run = vi.fn();
    renderHook(() => useGlobalKeys({}, run));
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    );
    expect(run).not.toHaveBeenCalled();
  });

  it("skips a non-allowlisted chord when focus is in a text input", () => {
    const run = vi.fn();
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "toggle-editor-mode" }, run));
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true, bubbles: true }),
    );
    expect(run).not.toHaveBeenCalled();
    input.remove();
  });

  it("still fires the palette/commit allowlist from inside an input", () => {
    const run = vi.fn();
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "open-palette" }, run));
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true }),
    );
    expect(run).toHaveBeenCalledWith("open-palette");
    input.remove();
  });

  it("skips a chord when focus is inside CodeMirror", () => {
    const run = vi.fn();
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "toggle-editor-mode" }, run));
    const cm = document.createElement("div");
    cm.className = "cm-editor";
    const inner = document.createElement("div");
    cm.appendChild(inner);
    document.body.appendChild(cm);
    inner.dispatchEvent(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true, bubbles: true }),
    );
    expect(run).not.toHaveBeenCalled();
    cm.remove();
  });

  it("does NOT fire tab navigation (Mod+1) from inside an input", () => {
    const run = vi.fn();
    const jump = vi.spyOn(cairnStore.getState(), "jumpToTab");
    renderHook(() => useGlobalKeys({}, run));
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "1", ctrlKey: true, bubbles: true }),
    );
    expect(jump).not.toHaveBeenCalled();
    input.remove();
    jump.mockRestore();
  });
});
