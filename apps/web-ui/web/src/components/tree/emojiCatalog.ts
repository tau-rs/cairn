export interface EmojiEntry {
  char: string;
  name: string;
  keywords: string[];
  group: string;
}

export const EMOJI_CATALOG: EmojiEntry[] = [
  // Frequently used
  {
    char: "📁",
    name: "folder",
    keywords: ["directory"],
    group: "Frequently used",
  },
  {
    char: "📚",
    name: "books",
    keywords: ["book", "library", "read"],
    group: "Frequently used",
  },
  {
    char: "🧠",
    name: "brain",
    keywords: ["think", "mind"],
    group: "Frequently used",
  },
  {
    char: "📝",
    name: "memo",
    keywords: ["note", "write"],
    group: "Frequently used",
  },
  {
    char: "⭐",
    name: "star",
    keywords: ["favorite"],
    group: "Frequently used",
  },
  {
    char: "🔥",
    name: "fire",
    keywords: ["hot", "trending"],
    group: "Frequently used",
  },
  {
    char: "💡",
    name: "bulb",
    keywords: ["idea", "light"],
    group: "Frequently used",
  },
  {
    char: "✅",
    name: "check",
    keywords: ["done", "task"],
    group: "Frequently used",
  },
  // Objects
  {
    char: "📦",
    name: "package",
    keywords: ["box", "archive"],
    group: "Objects",
  },
  {
    char: "📌",
    name: "pushpin",
    keywords: ["pin", "location"],
    group: "Objects",
  },
  {
    char: "🔖",
    name: "bookmark",
    keywords: ["save", "read"],
    group: "Objects",
  },
  {
    char: "📅",
    name: "calendar",
    keywords: ["date", "schedule"],
    group: "Objects",
  },
  {
    char: "🗂️",
    name: "dividers",
    keywords: ["files", "organize"],
    group: "Objects",
  },
  { char: "📊", name: "chart", keywords: ["graph", "data"], group: "Objects" },
  {
    char: "🧩",
    name: "puzzle",
    keywords: ["piece", "plugin"],
    group: "Objects",
  },
  { char: "🔗", name: "link", keywords: ["url", "chain"], group: "Objects" },
  {
    char: "⚙️",
    name: "gear",
    keywords: ["settings", "config"],
    group: "Objects",
  },
  {
    char: "🔑",
    name: "key",
    keywords: ["password", "secret"],
    group: "Objects",
  },
  {
    char: "💼",
    name: "briefcase",
    keywords: ["work", "business"],
    group: "Objects",
  },
  { char: "🎯", name: "target", keywords: ["goal", "aim"], group: "Objects" },
  {
    char: "🚀",
    name: "rocket",
    keywords: ["launch", "ship"],
    group: "Objects",
  },
  { char: "⏰", name: "alarm", keywords: ["clock", "time"], group: "Objects" },
  {
    char: "📷",
    name: "camera",
    keywords: ["photo", "picture"],
    group: "Objects",
  },
  // Symbols
  { char: "❤️", name: "heart", keywords: ["love", "like"], group: "Symbols" },
  { char: "⚡", name: "zap", keywords: ["fast", "energy"], group: "Symbols" },
  { char: "🏷️", name: "label", keywords: ["tag"], group: "Symbols" },
  { char: "❓", name: "question", keywords: ["help", "ask"], group: "Symbols" },
  {
    char: "❗",
    name: "exclamation",
    keywords: ["important", "alert"],
    group: "Symbols",
  },
  // Nature
  {
    char: "🌱",
    name: "seedling",
    keywords: ["plant", "grow", "new"],
    group: "Nature",
  },
  { char: "🌍", name: "globe", keywords: ["world", "earth"], group: "Nature" },
  { char: "☕", name: "coffee", keywords: ["break", "cafe"], group: "Nature" },
];

export function searchEmoji(query: string): EmojiEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return EMOJI_CATALOG;
  return EMOJI_CATALOG.filter(
    (e) => e.name.includes(q) || e.keywords.some((k) => k.includes(q)),
  );
}
