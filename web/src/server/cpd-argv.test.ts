import { describe, expect, it } from "vitest";
import { allCpdCommands, generateArgvFixturesJson, toArgv } from "./cpd-argv.ts";

describe("toArgv", () => {
  it("context.new creates a by-user Manager from the tree", () => {
    expect(toArgv({ kind: "context.new", contextId: "ctx-1", title: "My context", owner: "clayton-duarte" }))
      .toEqual(["context", "new", "ctx-1", "--title=My context", "--owner=clayton-duarte", "--by-user"]);
  });

  it("context.manager builds the context manager argv with --context and --by-user", () => {
    expect(toArgv({ kind: "context.manager", contextId: "ctx-1" }))
      .toEqual(["context", "manager", "--context=ctx-1", "--by-user"]);
  });

  it("session.send carries --webui-token only when it has one (contract §6.16)", () => {
    const send = { kind: "session.send", sessionId: "s1", text: "hi", unblocks: "u", origin: "webui" } as const;
    expect(toArgv(send)).toEqual(["session", "send", "s1", "--origin", "webui", "--unblocks", "u", "--", "hi"]);
    expect(toArgv({ ...send, webuiToken: "tok" })).toEqual(["session", "send", "s1", "--origin", "webui", "--webui-token", "tok", "--unblocks", "u", "--", "hi"]);
  });

  it("toArgv omits tui.attach", () => {
    // When tui.attach is omitted/removed from CpdCommand and toArgv, passing it must not produce ["tui", "--attach"]
    // (tui subcommand was removed in #1070 and commands.md §13.1).
    expect(() => toArgv({ kind: "tui.attach" } as any)).toThrow();
  });

  it("serializes allCpdCommands() to JSON fixture or passes to parser test", () => {
    const commands = allCpdCommands();
    const argvs = commands.map(toArgv);
    expect(argvs.length).toBeGreaterThan(0);
    // Fixture JSON representation of all WebUI argv patterns
    const json = generateArgvFixturesJson();
    expect(json).toBeDefined();
    const parsed = JSON.parse(json);
    expect(parsed).toEqual(argvs);
  });
});
