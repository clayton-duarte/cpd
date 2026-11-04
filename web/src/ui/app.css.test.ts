// @vitest-environment node
import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

const css = readFileSync(new URL("./app.css", import.meta.url), "utf8");

/** Declarations (property -> value) of every rule whose selector list contains `selector` exactly, merged in source order. */
function decls(selector: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (m[1] as string).split(",").map((s) => s.trim());
    if (!selectors.includes(selector)) continue;
    for (const d of (m[2] as string).split(";")) {
      const i = d.indexOf(":");
      if (i > 0) out.set(d.slice(0, i).trim(), d.slice(i + 1).trim().replace(/\s+/g, " "));
    }
  }
  return out;
}
/** Declarations of every rule whose selector text matches `re`, merged. */
function declsMatching(re: RegExp): Map<string, string> {
  const out = new Map<string, string>();
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (!re.test(m[1] as string)) continue;
    for (const d of (m[2] as string).split(";")) {
      const i = d.indexOf(":");
      if (i > 0) out.set(d.slice(0, i).trim(), d.slice(i + 1).trim().replace(/\s+/g, " "));
    }
  }
  return out;
}
function expectDecls(what: string, got: Map<string, string>, want: Record<string, string>): void {
  for (const [prop, value] of Object.entries(want)) expect(got.get(prop), `${what}: ${prop}`).toBe(value);
}

it("the shell is a bounded grid that does not scroll", () => {
  expectDecls("[data-shell]", decls("[data-shell]"), {
    display: "grid",
    height: "100vh",
    overflow: "hidden",
    "grid-template-rows": "auto minmax(0, 1fr) auto",
  });
  expect(decls("[data-shell] > *").get("min-height"), "[data-shell] > *: min-height").toBe("0");
});

it("the transcript log scrolls on its own", () => {
  expectDecls("[data-chat-dropzone]", decls("[data-chat-dropzone]"), {
    display: "flex",
    "flex-direction": "column",
    height: "100%",
    "min-height": "0",
    overflow: "hidden",
  });
  expectDecls('[data-part="transcript"]', decls('[data-part="transcript"]'), {
    position: "relative",
    flex: "1",
    "min-height": "0",
    display: "flex",
    "flex-direction": "column",
  });
  expectDecls('[role="log"]', decls('[role="log"]'), { flex: "1", "min-height": "0", "overflow-y": "auto" });
  expect(declsMatching(/jump-to-bottom/).get("position"), "jump button: position").toBe("absolute");
});

it("the context tree, plan and inbox scroll independently", () => {
  const aside = declsMatching(/\[data-shell\] aside/);
  expect(aside.get("overflow-y"), "shell aside: overflow-y").toBe("auto");
  expect(aside.get("height"), "shell aside: height").toBe("100%");
  const plan = declsMatching(/plan/i);
  expect(plan.get("overflow-y"), "plan pane: overflow-y").toBe("auto");
  expect(decls('[data-column="center"]').get("min-height"), "center column: min-height").toBe("0");
});

it("the shell grid is a single minmax column and the column fills its panel", () => {
  expect(decls("[data-shell]").get("grid-template-columns"), "shell: grid-template-columns").toBe("minmax(0, 1fr)");
  expect(decls("[data-column]").get("height"), "column: height").toBe("100%");
});

