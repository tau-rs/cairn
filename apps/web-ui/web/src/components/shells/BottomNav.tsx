import { Folder, FileText, Search, Share2, MoreHorizontal } from "lucide-react";
import type { ComponentType } from "react";
import type { MobileTab } from "../../store/store";

const TABS: {
  id: MobileTab;
  label: string;
  Icon: ComponentType<{ size?: number; "aria-hidden"?: boolean }>;
}[] = [
  { id: "files", label: "Files", Icon: Folder },
  { id: "editor", label: "Editor", Icon: FileText },
  { id: "search", label: "Search", Icon: Search },
  { id: "graph", label: "Graph", Icon: Share2 },
  { id: "more", label: "More", Icon: MoreHorizontal },
];

/** The mobile bottom tab bar. Stateless — parent owns `active`. */
export function BottomNav({
  active,
  onSelect,
}: {
  active: MobileTab;
  onSelect: (tab: MobileTab) => void;
}) {
  return (
    <nav className="flex shrink-0 border-t border-border bg-surface pb-[env(safe-area-inset-bottom)]">
      {TABS.map(({ id, label, Icon }) => (
        <button
          key={id}
          type="button"
          aria-label={label}
          aria-current={active === id ? "page" : undefined}
          onClick={() => onSelect(id)}
          className={
            "flex min-h-[44px] flex-1 flex-col items-center justify-center gap-0.5 py-1.5 text-[10px] " +
            (active === id ? "text-accent" : "text-faint")
          }
        >
          <Icon size={18} aria-hidden={true} />
          {label}
        </button>
      ))}
    </nav>
  );
}
