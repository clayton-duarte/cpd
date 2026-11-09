import { describe, expect, it } from "vitest";
import { toArgv } from "./cpd-argv.ts";

describe("toArgv", () => {
  it("context.new builds the context new argv", () => {
    // CONTRACT 20.1: the TUI's own argv for Action::NewContext (`tui/src/tree/actions.rs`).
    expect(toArgv({ kind: "context.new", contextId: "ctx-1", title: "My context", owner: "clayton-duarte" } as never))
      .toEqual(["context", "new", "ctx-1", "--title=My context", "--owner=clayton-duarte"]);
  });
});
