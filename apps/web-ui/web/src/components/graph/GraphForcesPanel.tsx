import { type ForceSettings, FORCE_RANGES } from "./forceSettings";

const SLIDERS: { key: keyof typeof FORCE_RANGES; label: string }[] = [
  { key: "center", label: "Center force" },
  { key: "repel", label: "Repel force" },
  { key: "linkForce", label: "Link force" },
  { key: "linkDistance", label: "Link distance" },
];

export function GraphForcesPanel(props: {
  settings: ForceSettings;
  onChange: (next: ForceSettings) => void;
  onReset: () => void;
}) {
  const { settings, onChange, onReset } = props;
  return (
    <div className="w-52 rounded-lg border border-border bg-surface p-3 shadow-2xl">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-wide text-faint">
          Forces
        </span>
        <button
          type="button"
          className="text-[11px] text-muted hover:text-text"
          onClick={onReset}
        >
          Reset
        </button>
      </div>
      {SLIDERS.map(({ key, label }) => {
        const range = FORCE_RANGES[key];
        return (
          <div key={key} className="mb-2.5">
            <div className="mb-1 flex justify-between text-[11px] text-text">
              <span>{label}</span>
              <span className="text-faint">{settings[key]}</span>
            </div>
            <input
              type="range"
              aria-label={label}
              className="w-full accent-accent"
              min={range.min}
              max={range.max}
              step={range.step}
              value={settings[key]}
              onChange={(e) =>
                onChange({ ...settings, [key]: Number(e.target.value) })
              }
            />
          </div>
        );
      })}
      <label className="mt-1 flex items-center gap-2 text-[11px] text-text">
        <input
          type="checkbox"
          aria-label="Freeze layout"
          className="accent-accent"
          checked={settings.frozen}
          onChange={(e) => onChange({ ...settings, frozen: e.target.checked })}
        />
        Freeze layout
      </label>
    </div>
  );
}
