import type { ReactNode } from "react";

/** The four leaf regions every shell composes. */
export interface ShellRegions {
  topBar: ReactNode;
  list: ReactNode;
  editor: ReactNode;
  backlinks: ReactNode;
  /** Optional docked ask panel (desktop shell). */
  ask?: ReactNode;
  /** Optional docked recovery panel (desktop shell). */
  recovery?: ReactNode;
}
