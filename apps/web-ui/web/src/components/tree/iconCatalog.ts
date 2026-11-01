import {
  Folder,
  FileText,
  Star,
  Bookmark,
  Tag,
  Calendar,
  Clock,
  CheckCircle,
  Flag,
  Heart,
  Lightbulb,
  Zap,
  Target,
  Rocket,
  Box,
  Pin,
  Link,
  Hash,
  Layers,
  Code,
  Terminal,
  Database,
  Settings,
  Search,
  Music,
  Image,
  Coffee,
  Briefcase,
  type LucideIcon,
} from "lucide-react";

export interface CatalogIcon {
  name: string;
  Component: LucideIcon;
  keywords: string[];
}

export const ICON_CATALOG: CatalogIcon[] = [
  { name: "folder", Component: Folder, keywords: ["directory"] },
  { name: "file", Component: FileText, keywords: ["document", "note"] },
  { name: "star", Component: Star, keywords: ["favorite", "important"] },
  { name: "bookmark", Component: Bookmark, keywords: ["save", "read"] },
  { name: "tag", Component: Tag, keywords: ["label"] },
  { name: "calendar", Component: Calendar, keywords: ["date", "schedule"] },
  { name: "clock", Component: Clock, keywords: ["time", "recent"] },
  { name: "check", Component: CheckCircle, keywords: ["done", "task", "todo"] },
  { name: "flag", Component: Flag, keywords: ["milestone", "priority"] },
  { name: "heart", Component: Heart, keywords: ["love", "like"] },
  { name: "idea", Component: Lightbulb, keywords: ["lightbulb", "think"] },
  { name: "zap", Component: Zap, keywords: ["fast", "energy", "action"] },
  { name: "target", Component: Target, keywords: ["goal", "aim"] },
  { name: "rocket", Component: Rocket, keywords: ["launch", "ship"] },
  { name: "box", Component: Box, keywords: ["package", "archive"] },
  { name: "pin", Component: Pin, keywords: ["map", "location"] },
  { name: "link", Component: Link, keywords: ["url", "reference"] },
  { name: "hash", Component: Hash, keywords: ["number", "tag"] },
  { name: "layers", Component: Layers, keywords: ["stack", "group"] },
  { name: "code", Component: Code, keywords: ["dev", "snippet"] },
  { name: "terminal", Component: Terminal, keywords: ["shell", "cli"] },
  { name: "database", Component: Database, keywords: ["data", "store"] },
  { name: "settings", Component: Settings, keywords: ["gear", "config"] },
  { name: "search", Component: Search, keywords: ["find", "magnify"] },
  { name: "music", Component: Music, keywords: ["audio", "song"] },
  { name: "image", Component: Image, keywords: ["photo", "picture"] },
  { name: "coffee", Component: Coffee, keywords: ["break", "cafe"] },
  { name: "work", Component: Briefcase, keywords: ["job", "business"] },
];

const BY_NAME = new Map(ICON_CATALOG.map((i) => [i.name, i.Component]));

export function iconByName(name: string): LucideIcon {
  return BY_NAME.get(name) ?? FileText;
}

export function searchIcons(query: string): CatalogIcon[] {
  const q = query.trim().toLowerCase();
  if (!q) return ICON_CATALOG;
  return ICON_CATALOG.filter(
    (i) => i.name.includes(q) || i.keywords.some((k) => k.includes(q)),
  );
}
