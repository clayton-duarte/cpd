// Red tests pinning the web shell's dark theme to the Dango v3 palette (web/README.md
// "### The Dango theme", hs-d836). The theme is a file in this repo, so the test reads
// web/src/index.css with node:fs and names each token in its failure message. RED now: the
// `.dark` block still holds the pre-Dango oklch values, so every assertion below fails until
// the impl Job writes the palette. No jsdom, no browser, no network: the stylesheet is a file.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const stylesheet = readFileSync(new URL("./index.css", import.meta.url), "utf8");

/** The Dango v3 palette, one row per `.dark` custom property (web/README.md's table). */
const DARK_PALETTE: ReadonlyArray<readonly [string, string]> = [
  ["--background", "#1c1a20"],
  ["--foreground", "#d8d2cc"],
  ["--card", "#24222a"],
  ["--card-foreground", "#d8d2cc"],
  ["--popover", "#24222a"],
  ["--popover-foreground", "#d8d2cc"],
  ["--primary", "#7aa2ce"],
  ["--primary-foreground", "#1c1a20"],
  ["--secondary", "#2a2730"],
  ["--secondary-foreground", "#d8d2cc"],
  ["--muted", "#2a2730"],
  ["--muted-foreground", "#aba3af"],
  ["--accent", "#2a2730"],
  ["--accent-foreground", "#d8d2cc"],
  ["--destructive", "#e17f6e"],
  ["--border", "#5a504a"],
  ["--input", "#5a504a"],
  ["--ring", "#7aa2ce"],
  ["--chart-1", "#7aa2ce"],
  ["--chart-2", "#8da96e"],
  ["--chart-3", "#c29653"],
  ["--chart-4", "#ca88a3"],
  ["--chart-5", "#67ada7"],
  ["--sidebar", "#1c1a20"],
  ["--sidebar-foreground", "#d8d2cc"],
  ["--sidebar-primary", "#7aa2ce"],
  ["--sidebar-primary-foreground", "#1c1a20"],
  ["--sidebar-accent", "#2a2730"],
  ["--sidebar-accent-foreground", "#d8d2cc"],
  ["--sidebar-border", "#5a504a"],
  ["--sidebar-ring", "#7aa2ce"],
];

/** The capture group 1 of the first match, or a named error naming the pattern. */
function firstCapture(source: string, pattern: RegExp): string {
  const captured = pattern.exec(source)?.[1];
  if (captured === undefined) throw new Error(`no match for ${pattern} in web/src/index.css`);
  return captured;
}

/** Every `property: value;` declaration in a rule body, value normalized to lowercase. */
function declarations(body: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const chunk of body.split(";")) {
    const [, property, value] = /^\s*([-\w]+)\s*:\s*(.+?)\s*$/.exec(chunk) ?? [];
    if (property !== undefined && value !== undefined) found.set(property, value.toLowerCase());
  }
  return found;
}

test("every dark-theme custom property equals its Dango v3 value", () => {
  const found = declarations(firstCapture(stylesheet, /(?:^|\n)\s*\.dark\s*\{([^}]*)\}/));
  for (const [token, expected] of DARK_PALETTE) {
    const actual = found.get(token) ?? "<not declared>";
    expect(actual, `${token}: expected ${expected}, found ${actual}`).toBe(expected);
  }
});

test("the dark theme's selection pair is #5a504a over #d8d2cc", () => {
  const found = declarations(firstCapture(stylesheet, /\.dark\s*::selection\s*\{([^}]*)\}/));
  const background = found.get("background") ?? found.get("background-color") ?? "<not declared>";
  const color = found.get("color") ?? "<not declared>";
  expect(background, `.dark ::selection background: expected #5a504a, found ${background}`).toBe(
    "#5a504a",
  );
  expect(color, `.dark ::selection color: expected #d8d2cc, found ${color}`).toBe("#d8d2cc");
});
