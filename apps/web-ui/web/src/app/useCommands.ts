import { useMemo } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { cairnStore, useCairn } from "./cairnStore";
import { toggleViewTarget } from "./routes";
import {
  COMMAND_DEFS,
  effectiveBinding,
  chordToId,
} from "../components/shortcuts/commands";
import { formatChord } from "../components/shortcuts/keybinding";
import {
  toPaletteCommands,
  parsePluginCommandId,
  pluginCommandArgs,
} from "../components/plugins/pluginCommands";
import type { PaletteCommand } from "../components/command-palette/CommandPalette";

const IS_MAC =
  typeof navigator !== "undefined" &&
  /mac/i.test(navigator.platform || navigator.userAgent || "");

/**
 * Builds the command-palette command list, the chord→id map for global key
 * dispatch, and the `runCommand` dispatcher. Dialog-opening routes through the
 * store's `setUi`; navigation routes through react-router. State the dispatcher
 * only reads at call time (`editorMode`, `activePath`) is pulled lazily via
 * `getState()` so this hook subscribes to nothing high-frequency.
 */
export function useCommands(): {
  commands: PaletteCommand[];
  chordMap: Record<string, string>;
  runCommand: (id: string) => void;
} {
  const navigate = useNavigate();
  const location = useLocation();
  const overrides = useCairn((s) => s.ui.keybindingOverrides);
  const plugins = useCairn((s) => s.plugins);

  const chordMap = useMemo(() => chordToId(overrides), [overrides]);

  const commands = useMemo<PaletteCommand[]>(
    () => [
      ...COMMAND_DEFS.filter((d) => d.id !== "open-palette").map((d) => {
        const eff = effectiveBinding(d.id, overrides);
        return {
          id: d.id,
          label: d.label,
          hint: eff ? formatChord(eff, IS_MAC) : undefined,
        };
      }),
      ...toPaletteCommands(plugins),
    ],
    [overrides, plugins],
  );

  const runCommand = (id: string) => {
    const st = cairnStore.getState();
    const pluginCmd = parsePluginCommandId(id);
    if (pluginCmd) {
      const args = pluginCommandArgs(st.plugins)[id] ?? null;
      void st.invokePlugin(pluginCmd.plugin, pluginCmd.command, args);
      st.setUi({ paletteOpen: false });
      return;
    }
    switch (id) {
      case "open-palette":
        st.setUi({ paletteOpen: !st.ui.paletteOpen });
        return;
      case "new-note":
        st.setUi({ newNoteInitial: "", newNoteOpen: true });
        break;
      case "close-tab":
        st.closeActiveTab();
        break;
      case "split-right":
        st.splitPane();
        break;
      case "close-pane":
        st.closePane();
        break;
      case "toggle-view":
        navigate(toggleViewTarget(location, st.activePath));
        break;
      case "open-settings":
        st.setUi({ settingsOpen: true });
        break;
      case "show-history":
        st.showHistory();
        break;
      case "recover-lost-work":
        if (st.activePath) st.openRecovery(st.activePath);
        break;
      case "open-ask":
        st.askOpen();
        break;
      case "toggle-editor-mode":
        st.setSettings({
          editorMode:
            st.settings.editorMode === "livepreview" ? "source" : "livepreview",
        });
        break;
      case "nav-back":
        navigate(-1);
        break;
      case "nav-forward":
        navigate(1);
        break;
    }
    st.setUi({ paletteOpen: false });
  };

  return { commands, chordMap, runCommand };
}
