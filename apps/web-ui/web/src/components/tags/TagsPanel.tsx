import { useState } from "react";
import { SectionLabel } from "../ui/SectionLabel";
import type { TagCount } from "../../contract";

export function TagsPanel(props: {
  tags: TagCount[];
  activeTag: string | null;
  onSelect: (tag: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  if (props.tags.length === 0) return null;
  return (
    <div className="mt-3 flex flex-col gap-0.5 text-sm">
      <button
        type="button"
        aria-label="toggle tags"
        className="mb-1 flex items-center gap-1 text-left"
        onClick={() => setCollapsed((c) => !c)}
      >
        <span aria-hidden="true" className="text-faint">
          {collapsed ? "▸" : "▾"}
        </span>
        <SectionLabel>Tags</SectionLabel>
      </button>
      {!collapsed &&
        props.tags.map((t) => {
          const active = t.tag === props.activeTag;
          return (
            <button
              key={t.tag}
              type="button"
              aria-label={`filter by tag ${t.tag}`}
              aria-pressed={active}
              className={
                "flex items-center justify-between rounded px-2 py-1 text-left " +
                (active
                  ? "bg-surface-2 text-text"
                  : "text-muted hover:bg-surface-2 hover:text-text")
              }
              onClick={() => props.onSelect(t.tag)}
            >
              <span className="truncate">{t.tag}</span>
              <span className="ml-2 text-faint">{t.count}</span>
            </button>
          );
        })}
    </div>
  );
}
