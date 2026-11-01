import { Eye, EyeOff } from "lucide-react";
import type { ColorGroup } from "./colorGroups";
import type { FilterSettings } from "./graphFilter";
import { type RecencySettings, RECENCY_WINDOW_RANGE } from "./recency";
import type { SuggestionsSettings } from "./suggestionsOverlay";

const norm = (q: string) => q.trim().toLowerCase();

/** Eye toggle: pressed = hidden (crossed-out eye). */
function EyeToggle(props: {
  label: string;
  hidden: boolean;
  disabled?: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={props.label}
      aria-pressed={props.hidden}
      disabled={props.disabled}
      className="flex-none text-faint hover:text-text disabled:opacity-40"
      onClick={props.onToggle}
    >
      {props.hidden ? <EyeOff size={13} /> : <Eye size={13} />}
    </button>
  );
}

export function GraphGroupsPanel(props: {
  groups: ColorGroup[];
  onChange: (next: ColorGroup[]) => void;
  filter: FilterSettings;
  onFilterChange: (next: FilterSettings) => void;
  recency: RecencySettings;
  onRecencyChange: (next: RecencySettings) => void;
  suggestions: SuggestionsSettings;
  onSuggestionsChange: (next: SuggestionsSettings) => void;
}) {
  const {
    groups,
    onChange,
    filter,
    onFilterChange,
    recency,
    onRecencyChange,
    suggestions,
    onSuggestionsChange,
  } = props;
  const update = (i: number, patch: Partial<ColorGroup>) =>
    onChange(groups.map((g, j) => (j === i ? { ...g, ...patch } : g)));
  const remove = (i: number) => onChange(groups.filter((_, j) => j !== i));
  const add = () =>
    onChange([...groups, { kind: "path", query: "", color: "#6366f1" }]);

  const hiddenSet = new Set(filter.hiddenGroupQueries.map(norm));
  const toggleGroup = (query: string) => {
    const next = hiddenSet.has(norm(query))
      ? filter.hiddenGroupQueries.filter((x) => norm(x) !== norm(query))
      : [...filter.hiddenGroupQueries, query.trim()];
    onFilterChange({ ...filter, hiddenGroupQueries: next });
  };

  return (
    <div className="w-52 rounded-lg border border-border bg-surface p-3 shadow-2xl">
      <div className="mb-2 text-[10px] uppercase tracking-wide text-faint">
        Groups
      </div>
      {groups.map((g, i) => (
        <div key={i} className="mb-2 flex items-center gap-1.5">
          <EyeToggle
            label={`Toggle group ${g.query || "(empty)"}`}
            hidden={hiddenSet.has(norm(g.query))}
            disabled={!g.query.trim()}
            onToggle={() => toggleGroup(g.query)}
          />
          <select
            aria-label="Group kind"
            className="rounded border border-border bg-bg px-1 py-0.5 text-[11px] text-text"
            value={g.kind}
            onChange={(e) =>
              update(i, { kind: e.target.value as ColorGroup["kind"] })
            }
          >
            <option value="path">Path</option>
            <option value="tag">Tag</option>
          </select>
          <input
            type="text"
            aria-label="Group query"
            className="min-w-0 flex-1 rounded border border-border bg-bg px-1.5 py-0.5 text-[11px] text-text"
            value={g.query}
            onChange={(e) => update(i, { query: e.target.value })}
          />
          <input
            type="color"
            aria-label="Group color"
            className="h-5 w-5 flex-none rounded border border-border bg-bg"
            value={g.color}
            onChange={(e) => update(i, { color: e.target.value })}
          />
          <button
            type="button"
            aria-label="Remove group"
            className="flex-none text-faint hover:text-text"
            onClick={() => remove(i)}
          >
            ×
          </button>
        </div>
      ))}
      {/* Ungrouped / "other" row — hides notes matching no group. */}
      <div className="mb-2 flex items-center gap-1.5">
        <EyeToggle
          label="Toggle ungrouped notes"
          hidden={filter.hideUngrouped}
          onToggle={() =>
            onFilterChange({ ...filter, hideUngrouped: !filter.hideUngrouped })
          }
        />
        <span className="flex-1 text-[11px] text-muted">Ungrouped</span>
      </div>
      <button
        type="button"
        className="text-[11px] text-accent hover:underline"
        onClick={add}
      >
        + Add group
      </button>

      {/* Min-degree filter */}
      <div className="mt-3 border-t border-border pt-3">
        <div className="flex items-center justify-between gap-2 text-[11px] text-text">
          <span>Min degree</span>
          <span className="text-faint">{filter.minDegree}</span>
        </div>
        <input
          type="range"
          aria-label="Minimum degree"
          className="w-full accent-accent"
          min={0}
          max={10}
          step={1}
          value={filter.minDegree}
          onChange={(e) =>
            onFilterChange({ ...filter, minDegree: Number(e.target.value) })
          }
        />
      </div>

      {/* Recency ring */}
      <div className="mt-3 border-t border-border pt-3">
        <label className="flex items-center gap-2 text-[11px] text-text">
          <input
            type="checkbox"
            className="accent-accent"
            checked={recency.enabled}
            onChange={(e) =>
              onRecencyChange({ ...recency, enabled: e.target.checked })
            }
          />
          Recent edits
        </label>
        {recency.enabled && (
          <div className="mt-2">
            <div className="flex items-center justify-between gap-2 text-[11px] text-text">
              <span>Window</span>
              <span className="text-faint">{recency.windowDays}d</span>
            </div>
            <input
              type="range"
              aria-label="Recency window (days)"
              className="w-full accent-accent"
              min={RECENCY_WINDOW_RANGE.min}
              max={RECENCY_WINDOW_RANGE.max}
              step={RECENCY_WINDOW_RANGE.step}
              value={recency.windowDays}
              onChange={(e) =>
                onRecencyChange({
                  ...recency,
                  windowDays: Number(e.target.value),
                })
              }
            />
          </div>
        )}
      </div>

      {/* Suggested links overlay */}
      <div className="mt-3 border-t border-border pt-3">
        <label className="flex items-center gap-2 text-[11px] text-text">
          <input
            type="checkbox"
            aria-label="Suggested links"
            className="accent-accent"
            checked={suggestions.enabled}
            onChange={(e) => onSuggestionsChange({ enabled: e.target.checked })}
          />
          Suggested links
        </label>
      </div>
    </div>
  );
}
