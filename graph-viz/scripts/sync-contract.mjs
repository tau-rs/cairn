// Vendors ts-rs bindings from the engine contract into src/contract/.
// Run whenever the contract changes: `pnpm sync-contract`.
import { readdirSync, mkdirSync, copyFileSync, cpSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = resolve(here, "../../crates/cairn-contract/bindings");
const dst = resolve(here, "../src/contract");

if (!existsSync(src)) {
  console.error(`contract bindings not found at ${src}`);
  process.exit(1);
}
mkdirSync(dst, { recursive: true });
for (const entry of readdirSync(src, { withFileTypes: true })) {
  const from = resolve(src, entry.name);
  const to = resolve(dst, entry.name);
  if (entry.isDirectory()) cpSync(from, to, { recursive: true });
  else if (entry.name.endsWith(".ts")) copyFileSync(from, to);
}
console.log(`synced contract bindings → ${dst}`);
