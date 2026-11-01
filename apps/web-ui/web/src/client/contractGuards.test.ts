import { describe, it, expect } from "vitest";
import {
  assertEvent,
  assertCommandResponse,
  assertQueryResponse,
  assertAnswerEvent,
  assertCollabServerMsg,
  ContractShapeError,
} from "./contractGuards";

describe("contractGuards", () => {
  it("passes a known-tag event through unchanged", () => {
    const e = { type: "committed", commit: "c1" };
    expect(assertEvent(e)).toBe(e);
  });
  it("passes known-tag command/query responses through", () => {
    const c = { type: "done" };
    const q = { type: "paths", paths: ["a.md"] };
    expect(assertCommandResponse(c)).toBe(c);
    expect(assertQueryResponse(q)).toBe(q);
  });
  it("rejects an unknown tag with a clear ContractShapeError", () => {
    expect(() => assertEvent({ type: "bogus" })).toThrow(ContractShapeError);
    expect(() => assertEvent({ type: "bogus" })).toThrow(/event/);
    expect(() => assertEvent({ type: "bogus" })).toThrow(/bogus/);
  });
  it("rejects a missing/invalid type, null, and non-objects", () => {
    expect(() => assertCommandResponse({})).toThrow(ContractShapeError);
    expect(() => assertQueryResponse(null)).toThrow(ContractShapeError);
    expect(() => assertEvent("nope")).toThrow(ContractShapeError);
    expect(() => assertEvent({ type: 5 })).toThrow(ContractShapeError);
  });
});

describe("assertAnswerEvent", () => {
  it("accepts each known AnswerEvent tag", () => {
    for (const e of [
      { type: "sources", paths: ["a.md"] },
      { type: "text_delta", text: "hi" },
      { type: "tool_started", tool: "search" },
      { type: "tool_completed", tool: "search", ok: true },
      { type: "turn_completed" },
      { type: "completed" },
      { type: "failed", message: "boom" },
    ]) {
      expect(assertAnswerEvent(e)).toBe(e);
    }
  });

  it("rejects an unknown tag", () => {
    expect(() => assertAnswerEvent({ type: "nope" })).toThrow(
      ContractShapeError,
    );
  });

  it("rejects a non-object", () => {
    expect(() => assertAnswerEvent(null)).toThrow(ContractShapeError);
  });
});

describe("assertCollabServerMsg", () => {
  it("accepts each valid variant", () => {
    for (const type of ["joined", "snapshot", "op", "error", "recoverable"]) {
      expect(assertCollabServerMsg({ type, note: "n.md" }).type).toBe(type);
    }
  });
  it("rejects an unknown tag", () => {
    expect(() => assertCollabServerMsg({ type: "bogus" })).toThrow(
      /Malformed collab server message/,
    );
  });
  it("rejects a non-object", () => {
    expect(() => assertCollabServerMsg(null)).toThrow(
      /Malformed collab server message/,
    );
  });
});
