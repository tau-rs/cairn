import { Modal } from "./ui/Modal";
import { Button } from "./ui/Button";
import { Settings } from "./Settings";
import { DaemonSettings } from "./DaemonSettings";
import { KeyboardShortcuts } from "./shortcuts/KeyboardShortcuts";
import { PluginsPanel } from "./plugins/PluginsPanel";
import { useCairn } from "../app/cairnStore";
import type { PluginSummary } from "../contract";
import type { Overrides } from "./shortcuts/commands";
import type { Settings as SettingsType } from "../store/store";

export function SettingsDialog({
  open,
  onOpenChange,
  settings,
  onChange,
  keybindingOverrides,
  onKeybindingsChange,
  plugins,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  settings: SettingsType;
  onChange: (patch: Partial<SettingsType>) => void;
  keybindingOverrides: Overrides;
  onKeybindingsChange: (o: Overrides) => void;
  plugins: PluginSummary[];
}) {
  const pluginDropped = useCairn((s) => s.pluginDropped);
  return (
    <Modal open={open} onClose={() => onOpenChange(false)} title="Settings">
      <Settings settings={settings} onChange={onChange} />
      <div className="my-3 border-t border-border" />
      <DaemonSettings />
      <div className="my-3 border-t border-border" />
      <KeyboardShortcuts
        overrides={keybindingOverrides}
        onChange={onKeybindingsChange}
      />
      <div className="my-3 border-t border-border" />
      <PluginsPanel plugins={plugins} dropped={pluginDropped} />
      <div className="mt-3 flex justify-end">
        <Button variant="secondary" onClick={() => onOpenChange(false)}>
          Done
        </Button>
      </div>
    </Modal>
  );
}
