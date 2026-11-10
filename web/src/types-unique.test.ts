import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const root = fileURLToPath(new URL("./", import.meta.url));
const generatedPath = join(root, "generated", "types.ts");
const NAMES = ["Cpd", "CpdExec", "CpdResult", "GitBoundary", "ResolvedSession", "SessionEmit", "SessionHandle", "SessionBackend", "CockpitUpdate", "CockpitSnapshot", "PendingSubmission"];

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return sources(p);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [p] : [];
  });
}

function extractGeneratedExports(filePath: string): Set<string> {
  const content = readFileSync(filePath, "utf8");
  const names = new Set<string>();
  const re = /^export\s+(?:type|interface|enum)\s+([A-Za-z0-9_$]+)\b/gm;
  let match: RegExpExecArray | null;
  while ((match = re.exec(content)) !== null) {
    const name = match[1];
    if (name) {
      names.add(name);
    }
  }
  return names;
}

interface DeclarationHit {
  file: string;
  name: string;
  kind: string;
}

function findDuplicateDeclarations(filePath: string, targetNames: Set<string>): DeclarationHit[] {
  const content = readFileSync(filePath, "utf8");
  const hits: DeclarationHit[] = [];
  const re = /^(?:export\s+)?(type|interface|enum)\s+([A-Za-z0-9_$]+)\b/gm;
  let match: RegExpExecArray | null;
  while ((match = re.exec(content)) !== null) {
    const kind = match[1];
    const name = match[2];
    if (kind && name && targetNames.has(name)) {
      hits.push({ file: filePath, name, kind });
    }
  }
  return hits;
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

  it("web client does not re-declare core generated types", () => {
    const generatedTypes = extractGeneratedExports(generatedPath);
    expect(generatedTypes.size).toBeGreaterThan(0);

    const generatedDir = join(root, "generated");
    const clientFiles = sources(root).filter((p) => !p.startsWith(generatedDir));

    const duplicates: DeclarationHit[] = [];
    for (const file of clientFiles) {
      duplicates.push(...findDuplicateDeclarations(file, generatedTypes));
    }

    const messages = duplicates.map(
      (d) => `${d.name} (${d.kind}) re-declared in ${d.file.slice(root.length)}`,
    );

    expect(messages, `Core generated types re-declared in web/src:\n${messages.join("\n")}`).toEqual([]);
  });
});
