import { describe, expect, it } from "vitest";
import { toArgv } from "./cpd-argv.ts";

describe("toArgv", () => {
  it("context.new builds the context new argv", () => {
    // CONTRACT 20.1: the TUI's own argv for Action::NewContext (`tui/src/tree/actions.rs`).
    expect(toArgv({ kind: "context.new", contextId: "ctx-1", title: "My context", owner: "clayton-duarte" } as never))
      .toEqual(["context", "new", "ctx-1", "--title=My context", "--owner=clayton-duarte"]);
  });

  it("session.send carries --webui-token only when it has one (contract §6.16)", () => {
    const send = { kind: "session.send", sessionId: "s1", text: "hi", unblocks: "u", origin: "webui" } as const;
    expect(toArgv(send)).toEqual(["session", "send", "s1", "--origin", "webui", "--unblocks", "u", "--", "hi"]);
    expect(toArgv({ ...send, webuiToken: "tok" })).toEqual(["session", "send", "s1", "--origin", "webui", "--webui-token", "tok", "--unblocks", "u", "--", "hi"]);
  });
});
