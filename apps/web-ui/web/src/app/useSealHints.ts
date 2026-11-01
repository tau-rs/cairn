import { useEffect } from "react";
import { cairnStore } from "./cairnStore";

/** U5 seal-now hints: ask the engine to seal a version when attention moves —
 *  note switch or window blur. The engine owns the policy (idle gap, backstop,
 *  skip-no-op); these are only boundary hints and are safe to over-send. */
export function useSealHints() {
  useEffect(() => {
    const unsub = cairnStore.subscribe((s, prev) => {
      if (s.activePath !== prev.activePath && prev.activePath !== null)
        void cairnStore.getState().sealNow();
    });
    const onBlur = () => void cairnStore.getState().sealNow();
    window.addEventListener("blur", onBlur);
    return () => {
      unsub();
      window.removeEventListener("blur", onBlur);
    };
  }, []);
}
