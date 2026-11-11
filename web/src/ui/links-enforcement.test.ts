// @vitest-environment node
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url)); // web/src/ui

// Every regular file under `dir`, recursively, in a stable order.
const walk = (dir: string): string[] => {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else if (entry.isFile() && (entry.name.endsWith(".tsx") || entry.name.endsWith(".ts"))) {
      // Exclude test files
      if (!entry.name.includes(".test.")) {
        out.push(full);
      }
    }
  }
  return out;
};

// Regex to detect raw `<a` JSX elements with target="_blank"
// or `<a` JSX elements whose href could be external.
// Specifically, ExternalLink is the only component allowed to render external anchor tags or target="_blank".
const RAW_EXTERNAL_ANCHOR = /<a\b[^>]*\btarget\s*=\s*["']_blank["'][^>]*>/;

describe("external links enforcement", () => {
  it("every external anchor in ui uses ExternalLink", () => {
    const files = walk(here);
    const offenders: string[] = [];

    for (const file of files) {
      const rel = relative(here, file);
      // ExternalLink.tsx is the only allowed site for raw external anchor / target="_blank"
      if (rel === "components/ExternalLink.tsx") continue;

      const content = readFileSync(file, "utf8");
      if (RAW_EXTERNAL_ANCHOR.test(content)) {
        offenders.push(rel);
      }
    }

    expect(
      offenders,
      `Files containing raw external <a> tags with target="_blank" instead of ExternalLink:\n${offenders.join("\n")}`,
    ).toEqual([]);
  });
});
