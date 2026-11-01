import type { Settings as SettingsType } from "../store/store";
import { SectionLabel } from "./ui/SectionLabel";

export function Settings(props: {
  settings: SettingsType;
  onChange: (patch: Partial<SettingsType>) => void;
}) {
  const s = props.settings;
  return (
    <div className="flex flex-col gap-2 text-sm text-text">
      <span className="mb-1">
        <SectionLabel>Privacy</SectionLabel>
      </span>
      <label className="flex items-center gap-2 text-muted">
        <input
          type="checkbox"
          checked={s.loadRemoteImages}
          onChange={(e) =>
            props.onChange({ loadRemoteImages: e.target.checked })
          }
        />
        Load remote images
      </label>
    </div>
  );
}
