import type { ReactNode } from "react";

export function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <span className="text-[10px] uppercase tracking-wide text-faint">
      {children}
    </span>
  );
}
