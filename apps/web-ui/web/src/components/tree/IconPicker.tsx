import { useState } from "react";
import type { ReactNode } from "react";
import * as Popover from "@radix-ui/react-popover";
import { searchEmoji } from "./emojiCatalog";
import { searchIcons } from "./iconCatalog";
import type { TreeItemStyle } from "./treeIcons";

/** Shared palette (theme-independent so it reads in dark/light/nord). */
const ICON_COLORS = [
  "#5b8def", // accent
  "#9ca0a8",
  "#e5484d",
  "#f5a623",
  "#30a46c",
  "#46b3e6",
  "#8e7bef",
  "#e668c3",
];

export function IconPicker({
  targetKind,
  value,
  onChange,
  trigger,
  open,
  onOpenChange,
}: {
  targetKind: "folder" | "note";
  value: TreeItemStyle;
  onChange: (style: TreeItemStyle) => void;
  trigger: ReactNode;
  /** Optional controlled open state — lets a context-menu item open the picker.
   *  When omitted, the popover is uncontrolled (the trigger toggles it). */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const [tab, setTab] = useState<"emoji" | "icons">("emoji");
  const [query, setQuery] = useState("");
  // Seed the color swatch from an existing lucide icon so reopening the picker
  // shows the icon's actual color selected, not the default.
  const [color, setColor] = useState(
    value.icon?.kind === "lucide" ? value.icon.color : ICON_COLORS[0],
  );

  const setIcon = (icon: TreeItemStyle["icon"]) => onChange({ ...value, icon });

  return (
    <Popover.Root
      open={open}
      onOpenChange={(o) => {
        onOpenChange?.(o);
        if (!o) setQuery("");
      }}
    >
      <Popover.Trigger asChild>{trigger}</Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="start"
          sideOffset={4}
          className="z-50 w-[300px] overflow-hidden rounded-xl border border-border bg-surface text-text shadow-2xl focus:outline-none"
        >
          <div className="flex items-center gap-1 px-2 pt-2">
            <button
              role="tab"
              aria-selected={tab === "emoji"}
              className={`rounded-t px-3 py-1.5 text-sm ${tab === "emoji" ? "bg-surface-2 text-text" : "text-muted"}`}
              onClick={() => setTab("emoji")}
            >
              Emoji
            </button>
            <button
              role="tab"
              aria-selected={tab === "icons"}
              className={`rounded-t px-3 py-1.5 text-sm ${tab === "icons" ? "bg-surface-2 text-text" : "text-muted"}`}
              onClick={() => setTab("icons")}
            >
              Icons
            </button>
            <span className="flex-1" />
            <button
              className="px-2 py-1 text-xs text-faint hover:text-danger"
              onClick={() => onChange({ ...value, icon: undefined })}
            >
              Remove
            </button>
          </div>

          <input
            className="m-2 w-[calc(100%-1rem)] rounded border border-border bg-surface-2 px-2.5 py-1.5 text-sm text-text outline-none focus:border-accent"
            placeholder={tab === "emoji" ? "Search emoji…" : "Search icons…"}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />

          {tab === "emoji" ? (
            <div className="grid max-h-[190px] grid-cols-8 gap-0.5 overflow-y-auto px-2 pb-2">
              {searchEmoji(query).map((e) => (
                <button
                  key={e.char}
                  aria-label={`${e.name} ${e.char}`}
                  className="flex aspect-square items-center justify-center rounded text-[17px] hover:bg-surface-2"
                  onClick={() => setIcon({ kind: "emoji", value: e.char })}
                >
                  {e.char}
                </button>
              ))}
            </div>
          ) : (
            <>
              <div className="px-3 pb-1 pt-0.5 text-[11px] uppercase tracking-wide text-faint">
                Icon color
              </div>
              <div className="flex flex-wrap gap-1.5 px-3 pb-2">
                {ICON_COLORS.map((c) => (
                  <button
                    key={c}
                    aria-label={`color ${c}`}
                    onClick={() => setColor(c)}
                    style={{ background: c }}
                    className={`h-5 w-5 rounded-full border-2 ${color === c ? "border-text" : "border-transparent"}`}
                  />
                ))}
              </div>
              <div className="grid max-h-[170px] grid-cols-7 gap-0.5 overflow-y-auto px-2 pb-2">
                {searchIcons(query).map(({ name, Component }) => (
                  <button
                    key={name}
                    aria-label={`icon ${name}`}
                    className="flex aspect-square items-center justify-center rounded hover:bg-surface-2"
                    onClick={() => setIcon({ kind: "lucide", name, color })}
                  >
                    <Component size={17} color={color} />
                  </button>
                ))}
              </div>
            </>
          )}

          {targetKind === "folder" && (
            <div className="border-t border-border px-3 py-2">
              <div className="pb-1 text-[11px] uppercase tracking-wide text-faint">
                Folder color
              </div>
              <div className="flex flex-wrap gap-1.5">
                <button
                  aria-label="folder color none"
                  onClick={() => onChange({ ...value, folderColor: undefined })}
                  className={`flex h-5 w-5 items-center justify-center rounded-full border-2 bg-surface-2 text-[11px] text-faint ${value.folderColor ? "border-transparent" : "border-text"}`}
                >
                  ∅
                </button>
                {/* skip accent + grey (indices 0–1): too low-contrast as a row tint */}
                {ICON_COLORS.slice(2).map((c) => (
                  <button
                    key={c}
                    aria-label={`folder color ${c}`}
                    onClick={() => onChange({ ...value, folderColor: c })}
                    style={{ background: c }}
                    className={`h-5 w-5 rounded-full border-2 ${value.folderColor === c ? "border-text" : "border-transparent"}`}
                  />
                ))}
              </div>
            </div>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
