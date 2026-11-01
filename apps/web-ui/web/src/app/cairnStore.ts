import { useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { createCairnStore, type CairnState } from "../store/store";
import { makeBackend } from "./makeBackend";

const { client, host } = makeBackend();
export const cairnStore = createCairnStore(client, host);
export const cairnClient = client;

export function useCairn<T>(selector: (s: CairnState) => T): T {
  return useStore(cairnStore, selector);
}

/** The action (method) subset of the store — every function-valued member. */
export type CairnActions = {
  [
    K in keyof CairnState as CairnState[K] extends (...args: never[]) => unknown
      ? K
      : never
  ]: CairnState[K];
};

const selectActions = (s: CairnState): CairnActions =>
  Object.fromEntries(
    Object.entries(s).filter(([, v]) => typeof v === "function"),
  ) as CairnActions;

/**
 * Reactively select every store action. Routes through the subscription model
 * like all other state (no ad-hoc `cairnStore.getState()` capture during
 * render); `useShallow` keeps the returned bag referentially stable across
 * renders, so consumers don't re-render when unrelated data slices change.
 */
export function useActions(): CairnActions {
  return useStore(cairnStore, useShallow(selectActions));
}
