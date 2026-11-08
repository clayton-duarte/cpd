// The web-to-CLI argv contract test (docs/contract.md §5.6): build every cpd argv the web
// sends from the one shared module, feed them all to the real, built `cpd argv-check`, and
// fail when the parser refuses any of them. argv-check is parse-only (no store, no network,
// no git). It is red until the argv-impl Jobs add the command and fill `allCpdCommands`.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { allCpdCommands, toArgv } from "../cpd-argv.ts";

const repoRoot = fileURLToPath(new URL("../../../../", import.meta.url));
const cpdBin = process.env["CPD_BIN"] ?? join(repoRoot, "target", "debug", "cpd");

interface Entry {
  readonly name: string;
  readonly argv: string[];
}

describe("cdp argv", () => {
  it("cdp argv parses against the real cpd parser", () => {
    const entries: Entry[] = allCpdCommands().map((cmd) => ({ name: cmd.kind, argv: toArgv(cmd) }));

    // Red against the empty stub: the union must cover the web's argv surface.
    expect(entries.length, "allCpdCommands() is empty").toBeGreaterThan(0);

    const argvList = entries.map((e) => e.argv);

    // The cockpit's `plan list` (§5.6, readmodel.ts) ...
    expect(
      argvList.some((argv) => argv.join(" ") === "plan list"),
      "missing the cockpit `plan list` command",
    ).toBe(true);
    // ... and the settings' `config tiers set` with all four role flags (the air-n37 bug).
    const tiers = argvList.find((argv) => argv.slice(0, 3).join(" ") === "config tiers set");
    expect(tiers, "missing the settings `config tiers set` command").toBeDefined();
    for (const role of ["auxiliar", "worker", "lead", "manager"]) {
      expect(tiers ?? [], `config tiers set is missing --${role}`).toContain(`--${role}`);
    }

    // A missing binary is a broken build, never a silent skip: fail loudly.
    if (!existsSync(cpdBin)) {
      throw new Error(`the cpd binary is missing at ${cpdBin}; build it with scripts/cargo.sh build --workspace`);
    }
    const reply = spawnSync(cpdBin, ["argv-check"], {
      input: JSON.stringify(argvList),
      encoding: "utf8",
      timeout: 30_000,
    });
    if (reply.error) throw reply.error;
    expect(reply.status, `${cpdBin} argv-check exited ${reply.status}: ${reply.stderr}`).toBe(0);
    expect(reply.stdout.trim(), `${cpdBin} argv-check stdout`).toBe('{"ok":true}');
  });
});
