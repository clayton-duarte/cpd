/**
 * The question widget's header (docs/commands.md 15.13, contract 6.1.1): the third-party package
 * @juicesharp/rpiv-ask-user-question@2.13.8 draws the single-question header as a padded chip; the adapter's Theme.bg
 * wrapper (installed as the jump hint's is, `installHint`) turns it into a grey read-only title pill. The package is not
 * a dependency: its call is reproduced. Assumption: the install point is `installHint` of lib/hint.ts.
 */
import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { installHint } from "../lib/hint.ts";

const THEME = new URL("../themes/dango.json", import.meta.url);
type ThemeLike = { fg(c: string, t: string): string; bg(c: string, t: string): string };
const pi = fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent"));
const themeMod = (await import(pathToFileURL(join(dirname(pi), "modes/interactive/theme/theme.js")).href)) as {
	loadThemeFromPath(path: string, mode?: string): ThemeLike;
};
const piMod = (await import(pathToFileURL(pi).href)) as { Theme: { prototype: ThemeLike } };
const load = (): ThemeLike => themeMod.loadThemeFromPath(THEME.pathname, "256color");
const original = piMod.Theme.prototype.bg;
const E = "\x1b[";
const PILL = `${E}37m\uE0B6${E}39m${E}47m${E}30mAssumptions${E}49m${E}39m${E}37m\uE0B4${E}39m`;
/** rpiv-ask-user-question@2.13.8 view/tab-content-strategy.ts:112: theme.bg("selectedBg", ` ${question.header} `) */
const header = (t: ThemeLike, h = "Assumptions") => t.bg("selectedBg", ` ${h} `);

test("the question's header badge is drawn as a grey pill with caps and no padding", () => {
	const t = load();
	console.log("today:", JSON.stringify(header(t)));
	const off = installHint();
	try {
		const out = header(t);
		assert.equal(out, PILL);
		assert.doesNotMatch(out, /\x1b\[(2|7|48;|38;)/);
	} finally {
		off();
	}
});

test("the pill is what the real widget's call produces: bg selectedBg of a padded header", () => {
	const t = load();
	const off = installHint();
	try {
		for (const h of ["Assumptions", "Scope", "Pick one"]) {
			assert.equal(header(t, h), `${E}37m\uE0B6${E}39m${E}47m${E}30m${h}${E}49m${E}39m${E}37m\uE0B4${E}39m`);
		}
	} finally {
		off();
	}
});

test("every other selectedBg caller is unchanged", () => {
	const t = load();
	const callers = {
		hint: (x: ThemeLike) => x.bg("selectedBg", x.fg("text", " ↓ Jump to latest message · Ctrl+End ")),
		// session-selector.js:431 and tree-selector.js:610 pass ANSI-bearing text
		selector: (x: ThemeLike) => x.bg("selectedBg", `${E}1m ${x.fg("accent", "session title")} ${E}22m`),
		tabBar: (x: ThemeLike) => x.bg("selectedBg", x.fg("text", " Assumptions ")),
	};
	const before = { selector: callers.selector(t), tabBar: callers.tabBar(t) };
	assert.match(before.selector, /\x1b\[48;5;4m/);
	assert.match(before.tabBar, /\x1b\[48;5;4m/);
	const off = installHint();
	try {
		assert.equal(callers.hint(t), `${E}90m ↓ Jump to latest message · Ctrl+End ${E}39m`);
		assert.equal(callers.selector(t), before.selector);
		assert.equal(callers.tabBar(t), before.tabBar);
		assert.equal(header(t), PILL, "while the header is the pill");
	} finally {
		off();
	}
});

test("the wrapper is idempotent and its removal restores pi's own bg", () => {
	const proto = piMod.Theme.prototype;
	const off1 = installHint();
	const wrapped = proto.bg;
	const off2 = installHint();
	try {
		assert.equal(proto.bg, wrapped, "a second install does not wrap again");
		assert.equal(header(load()), PILL);
	} finally {
		off2();
		off1();
	}
	assert.equal(proto.bg, original);
	assert.equal(header(load()), "\x1b[48;5;4m Assumptions \x1b[49m");
});
