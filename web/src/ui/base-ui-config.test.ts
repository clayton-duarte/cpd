// @vitest-environment node
import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

// The web/ base-registry switch (shadcn base = Base UI). The two config files are read as
// text next to this test (`web/src/ui/` -> repo `web/`), never through a network or the CLI:
// the switch is a config change, and these pins are its contract.
const readJson = (rel: string) =>
  JSON.parse(readFileSync(new URL(rel, import.meta.url), "utf8"));

it("components.json uses a base style and the real css entry", () => {
  const cfg = readJson("../../components.json");
  // A base style (base-nova, base-vega, ...) selects the Base UI registry; a radix-* or
  // aria-* style would not.
  expect(cfg.style).toMatch(/^base-/);
  // The stale field points at `src/index.css`, which does not exist; the live Tailwind
  // entry is `web/src/ui/app.css`.
  expect(cfg.tailwind.css).toBe("src/ui/app.css");
});

it("the base primitive package is installed", () => {
  const pkg = readJson("../../package.json");
  expect(Object.keys(pkg.dependencies ?? {})).toContain("@base-ui/react");
});
