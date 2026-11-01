import { Folder, FileText } from "lucide-react";
import { iconByName } from "./iconCatalog";
import type { TreeItemStyle } from "./treeIcons";

export function TreeItemIcon({
  kind,
  style,
}: {
  kind: "folder" | "note";
  style?: TreeItemStyle;
}) {
  const icon = style?.icon;

  if (icon?.kind === "emoji") {
    return (
      <span aria-hidden className="text-[15px] leading-none">
        {icon.value}
      </span>
    );
  }

  if (icon?.kind === "lucide") {
    const Cmp = iconByName(icon.name);
    return <Cmp aria-hidden size={16} color={icon.color} />;
  }

  if (kind === "folder") {
    // filled folder, single muted accent
    return (
      <Folder
        aria-hidden
        size={16}
        fill="currentColor"
        className="text-muted"
      />
    );
  }
  return <FileText aria-hidden size={16} className="text-faint" />;
}
