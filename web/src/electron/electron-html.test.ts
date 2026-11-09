// @vitest-environment node
import { readFileSync } from "node:fs";

test("electron.html forces the default cursor on the terminal and its canvas", () => {
  const html = readFileSync(new URL("../../electron.html", import.meta.url), "utf8");
  expect(html).toMatch(/#terminal\s*,\s*#terminal\s+canvas\s*\{\s*cursor\s*:\s*default\s*!important\s*;?\s*\}/);
});

test("html and body fill the viewport", () => {
  const html = readFileSync(new URL("../../electron.html", import.meta.url), "utf8");
  // Radix's scroll lock sets body[data-scroll-locked]{position:relative!important}, making body the
  // containing block for #webui-root/#terminal. With no height on html/body that box collapses to 0
  // and a dialog blanks the whole app (air-n74); height: 100% keeps the viewport box under the lock.
  expect(html).toMatch(/html\s*,\s*body\s*\{[^}]*height\s*:\s*100%\s*;?[^}]*\}/);
});

test("the webui root drops its tab-bar offset when the tab bar is hidden", () => {
  const html = readFileSync(new URL("../../electron.html", import.meta.url), "utf8");
  // The host tab bar only shows in TUI (air-d50); when it is hidden the WebUI root must not keep
  // its 30px top offset, or an empty strip sits above the app (air-n68). The rule keys on the
  // tab bar's own `hidden` attribute, so it follows the renderer's toggle without a mode attribute.
  expect(html).toMatch(/#tab-bar\[hidden\][^{}]*#webui-root\s*\{[^}]*top\s*:\s*0\s*(?:px)?\s*;?[^}]*\}/);
});

test("electron.html and index.html are titled CPD", () => {
  for (const file of ["../../electron.html", "../../index.html"]) {
    const html = readFileSync(new URL(file, import.meta.url), "utf8");
    expect(html).toMatch(/<title>CPD<\/title>/);
  }
});

test("tab bar nav starts hidden in the served HTML", () => {
  const html = readFileSync(new URL("../../electron.html", import.meta.url), "utf8");
  // WebUI and Preview never show the host tab strip; the nav must be hidden before the renderer starts.
  const nav = html.match(/<nav\b[^>]*\bid="tab-bar"[^>]*>/);
  expect(nav).not.toBeNull();
  expect(nav![0]).toMatch(/\shidden(?=[\s=>])/);
});
