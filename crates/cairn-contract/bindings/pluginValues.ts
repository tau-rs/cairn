// crates/cairn-contract/bindings/pluginValues.ts
// Hand-authored to accompany the ts-rs-generated PluginSlot/PluginWidget/PluginIcon
// types (ts-rs emits types only). Kept in lockstep with the Rust enums by a
// cairn-contract unit test. Vendored into web/src/contract by sync-contract.sh.
export const PLUGIN_SLOT_VALUES =
  ["sidebar.section", "topbar.action", "command", "panel.main"] as const;
export const PLUGIN_WIDGET_KIND_VALUES = ["text", "action", "list", "iframe"] as const;
export const PLUGIN_ICON_VALUES =
  ["tag", "search", "note", "folder", "link", "star", "info", "play"] as const;
