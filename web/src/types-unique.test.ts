import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const root = fileURLToPath(new URL("./", import.meta.url));
const NAMES = ["Cpd", "CpdExec", "CpdResult", "GitBoundary", "ResolvedSession", "SessionEmit", "SessionHandle", "SessionBackend", "CockpitUpdate", "CockpitSnapshot", "PendingSubmission"];

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = `${dir}${e.name}`;
    if (e.isDirectory()) return sources(`${p}/`);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [p] : [];
  });
}

describe("shared types", () => {
  it("each shared type is declared exactly once under web/src", () => {
    const files = sources(root).map((p) => ({ p, text: readFileSync(p, "utf8") }));
    const bad: string[] = [];
    for (const n of NAMES) {
      const re = new RegExp(`^(export )?(interface|type) ${n}\\b`, "m");
      const hits = files.filter((f) => re.test(f.text)).map((f) => f.p.slice(root.length));
      if (hits.length !== 1) bad.push(`${n}: ${hits.length} declarations [${hits.join(", ")}]`);
    }
    expect(bad, `types not declared exactly once: ${bad.join("; ")}`).toEqual([]);
  });
});
