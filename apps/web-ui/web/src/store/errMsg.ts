import type { ContractError } from "../contract";

/** Format an unknown error into a user-facing message. Handles ContractError
 *  (the tagged object the client rejects with) plus Error/anything else. Shared
 *  by store.ts and askSlice.ts so both surfaces format engine errors alike. */
export function errMsg(err: unknown): string {
  // ContractError (rejected by the client) is a tagged object.
  if (err && typeof err === "object" && "type" in err) {
    const e = err as ContractError;
    if (e.type === "not_found") return `Not found: ${e.what}`;
    return e.message;
  }
  return err instanceof Error ? err.message : String(err);
}
