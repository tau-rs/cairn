import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { GraphGroupsPanel } from "./GraphGroupsPanel";
import type { ColorGroup } from "./colorGroups";
import { DEFAULT_FILTER, type FilterSettings } from "./graphFilter";
import { DEFAULT_RECENCY, type RecencySettings } from "./recency";
import {
  DEFAULT_SUGGESTIONS_SETTINGS,
  type SuggestionsSettings,
} from "./suggestionsOverlay";

const groups: ColorGroup[] = [
  { kind: "path", query: "projects", color: "#6366f1" },
];

function renderPanel(over: {
  groups?: ColorGroup[];
  onChange?: () => void;
  filter?: FilterSettings;
  onFilterChange?: (f: FilterSettings) => void;
  recency?: RecencySettings;
  onRecencyChange?: (r: RecencySettings) => void;
  suggestions?: SuggestionsSettings;
  onSuggestionsChange?: (s: SuggestionsSettings) => void;
}) {
  const props = {
    groups,
    onChange: vi.fn(),
    filter: DEFAULT_FILTER,
    onFilterChange: vi.fn(),
    recency: DEFAULT_RECENCY,
    onRecencyChange: vi.fn(),
    suggestions: DEFAULT_SUGGESTIONS_SETTINGS,
    onSuggestionsChange: vi.fn(),
    ...over,
  };
  render(<GraphGroupsPanel {...props} />);
  return props;
}

describe("GraphGroupsPanel", () => {
  it("renders a row per group (kind, query, color)", () => {
    renderPanel({});
    expect(screen.getByDisplayValue("projects")).toBeInTheDocument();
    expect(screen.getByLabelText("Group kind")).toHaveValue("path");
  });
  it("Add group appends a default group", () => {
    const onChange = vi.fn();
    renderPanel({ groups: [], onChange });
    fireEvent.click(screen.getByRole("button", { name: /add group/i }));
    expect(onChange).toHaveBeenCalledWith([
      { kind: "path", query: "", color: "#6366f1" },
    ]);
  });
  it("editing the query fires onChange for that row", () => {
    const onChange = vi.fn();
    renderPanel({ onChange });
    fireEvent.change(screen.getByLabelText("Group query"), {
      target: { value: "journal" },
    });
    expect(onChange).toHaveBeenCalledWith([
      { kind: "path", query: "journal", color: "#6366f1" },
    ]);
  });
  it("changing the kind fires onChange", () => {
    const onChange = vi.fn();
    renderPanel({ onChange });
    fireEvent.change(screen.getByLabelText("Group kind"), {
      target: { value: "tag" },
    });
    expect(onChange).toHaveBeenCalledWith([
      { kind: "tag", query: "projects", color: "#6366f1" },
    ]);
  });
  it("remove drops the row", () => {
    const onChange = vi.fn();
    renderPanel({ onChange });
    fireEvent.click(screen.getByRole("button", { name: /remove group/i }));
    expect(onChange).toHaveBeenCalledWith([]);
  });

  it("the group eye toggle adds/removes the query from hiddenGroupQueries", () => {
    const onFilterChange = vi.fn();
    renderPanel({ onFilterChange });
    fireEvent.click(
      screen.getByRole("button", { name: /toggle group projects/i }),
    );
    expect(onFilterChange).toHaveBeenCalledWith({
      ...DEFAULT_FILTER,
      hiddenGroupQueries: ["projects"],
    });
  });
  it("un-hiding a group removes it from hiddenGroupQueries", () => {
    const onFilterChange = vi.fn();
    renderPanel({
      filter: { ...DEFAULT_FILTER, hiddenGroupQueries: ["projects"] },
      onFilterChange,
    });
    fireEvent.click(
      screen.getByRole("button", { name: /toggle group projects/i }),
    );
    expect(onFilterChange).toHaveBeenCalledWith({
      ...DEFAULT_FILTER,
      hiddenGroupQueries: [],
    });
  });
  it("the ungrouped eye toggle flips hideUngrouped", () => {
    const onFilterChange = vi.fn();
    renderPanel({ onFilterChange });
    fireEvent.click(
      screen.getByRole("button", { name: /toggle ungrouped notes/i }),
    );
    expect(onFilterChange).toHaveBeenCalledWith({
      ...DEFAULT_FILTER,
      hideUngrouped: true,
    });
  });
  it("the min-degree slider fires onFilterChange", () => {
    const onFilterChange = vi.fn();
    renderPanel({ onFilterChange });
    fireEvent.change(screen.getByLabelText("Minimum degree"), {
      target: { value: "3" },
    });
    expect(onFilterChange).toHaveBeenCalledWith({
      ...DEFAULT_FILTER,
      minDegree: 3,
    });
  });
  it("the recency checkbox enables the ring and reveals the window slider", () => {
    const onRecencyChange = vi.fn();
    const { rerender } = render(
      <GraphGroupsPanel
        groups={groups}
        onChange={vi.fn()}
        filter={DEFAULT_FILTER}
        onFilterChange={vi.fn()}
        recency={DEFAULT_RECENCY}
        onRecencyChange={onRecencyChange}
        suggestions={DEFAULT_SUGGESTIONS_SETTINGS}
        onSuggestionsChange={vi.fn()}
      />,
    );
    // window slider hidden while disabled
    expect(screen.queryByLabelText(/recency window/i)).toBeNull();
    fireEvent.click(screen.getByRole("checkbox", { name: /recent edits/i }));
    expect(onRecencyChange).toHaveBeenCalledWith({
      ...DEFAULT_RECENCY,
      enabled: true,
    });
    rerender(
      <GraphGroupsPanel
        groups={groups}
        onChange={vi.fn()}
        filter={DEFAULT_FILTER}
        onFilterChange={vi.fn()}
        recency={{ ...DEFAULT_RECENCY, enabled: true }}
        onRecencyChange={onRecencyChange}
        suggestions={DEFAULT_SUGGESTIONS_SETTINGS}
        onSuggestionsChange={vi.fn()}
      />,
    );
    expect(screen.getByLabelText(/recency window/i)).toBeInTheDocument();
  });
});

const baseProps = {
  groups: [],
  onChange: () => {},
  filter: { minDegree: 0, hiddenGroupQueries: [], hideUngrouped: false },
  onFilterChange: () => {},
  recency: { enabled: false, windowDays: 30 },
  onRecencyChange: () => {},
};

describe("GraphGroupsPanel suggested-links toggle", () => {
  it("reflects the enabled state and fires onSuggestionsChange when toggled", () => {
    const onSuggestionsChange = vi.fn();
    render(
      <GraphGroupsPanel
        {...baseProps}
        suggestions={{ enabled: false }}
        onSuggestionsChange={onSuggestionsChange}
      />,
    );
    const box = screen.getByLabelText("Suggested links") as HTMLInputElement;
    expect(box.checked).toBe(false);
    fireEvent.click(box);
    expect(onSuggestionsChange).toHaveBeenCalledWith({ enabled: true });
  });
});
