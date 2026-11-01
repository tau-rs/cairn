import { Shell } from "../Shell";
import { MobileShell } from "./MobileShell";
import { TabletShell } from "./TabletShell";
import { useBreakpoint } from "../responsive/useBreakpoint";
import type { ShellRegions } from "./regions";

/** Selects the layout shell for the active viewport tier. */
export function AppShell(props: ShellRegions) {
  const bp = useBreakpoint();
  if (bp === "mobile") return <MobileShell {...props} />;
  if (bp === "tablet") return <TabletShell {...props} />;
  return <Shell {...props} />;
}
