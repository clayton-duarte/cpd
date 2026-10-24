/**
 * The question widget's multi-question tab bar (docs/commands.md 15.13, contract 6.1.1 'Pill, colour n'): the third-party
 * package @juicesharp/rpiv-ask-user-question@2.13.8 draws it as ` ← ■ D4 … ✓ Submit →`; the adapter turns it into pills
 * joined by `─`. The package is not a dependency: its `view/components/tab-bar.ts` `TabBar` is reproduced below.
 * Assumptions: the install point is `installHint` of lib/hint.ts (as the header pill's is); the hook is on pi-tui's
 * `Container.prototype.render`; separators are `ESC[90m─ESC[39m` (colour 8, as the jump hint's grey).
 */
import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { Container, truncateToWidth, visibleWidth, type Component } from "@earendil-works/pi-tui";
import { installHint } from "../lib/hint.ts";

const THEME = new URL("../themes/dango.json", import.meta.url);
type ThemeLike = { fg(c: string, t: string): string; bg(c: string, t: string): string };
const pi = fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent"));
const themeMod = (await import(pathToFileURL(join(dirname(pi), "modes/interactive/theme/theme.js")).href)) as {
	loadThemeFromPath(path: string, mode?: string): ThemeLike;
};
const theme = themeMod.loadThemeFromPath(THEME.pathname, "256color");
const E = "\x1b[";
const SEP = `${E}90m─${E}39m`;
const pill = (n: number, label: string): string =>
	`${E}3${n}m\uE0B6${E}7m${label}${E}27m${E}3${n}m\uE0B4${E}39m`;
const BLUE = 4;
const GREEN = 2;
const GREY = 7;

type Props = {
	tabs: ReadonlyArray<{ label: string; answered: boolean; active: boolean }>;
	submit: { active: boolean; allAnswered: boolean };
};

/** rpiv-ask-user-question@2.13.8 view/components/tab-bar.ts, `TabBar.render`, reproduced. */
class TabBar implements Component {
	props: Props;
	private readonly theme: ThemeLike;
	constructor(theme: ThemeLike) {
		this.theme = theme;
		this.props = { tabs: [], submit: { active: false, allAnswered: false } };
	}
	setProps(props: Props): void {
		this.props = props;
	}
	invalidate(): void {}
	render(width: number): string[] {
		const pieces: string[] = [" ← "];
		for (const tab of this.props.tabs) {
			const box = tab.answered ? "■" : "□";
			const rawSeg = ` ${box} ${tab.label} `;
			const styled = tab.active
				? this.theme.bg("selectedBg", this.theme.fg("text", rawSeg))
				: this.theme.fg(tab.answered ? "success" : "muted", rawSeg);
			pieces.push(styled);
			pieces.push(" ");
		}
		const submitText = " ✓ Submit ";
		const submitStyled = this.props.submit.active
			? this.theme.bg("selectedBg", this.theme.fg("text", submitText))
			: this.theme.fg(this.props.submit.allAnswered ? "success" : "dim", submitText);
		pieces.push(submitStyled);
		pieces.push(" →");
		const tabLine = truncateToWidth(pieces.join(""), width, "");
		return [tabLine, ""];
	}
}

class Other implements Component {
	invalidate(): void {}
	render(): string[] {
		return [" ← x →"];
	}
}

const tab = (label: string, answered: boolean, active: boolean) => ({ label, answered, active });
const D = (props: Props, width = 120): string[] => {
	const bar = new TabBar(theme);
	bar.setProps(props);
	const box = new Container();
	box.addChild(bar);
	box.addChild(new Other());
	return box.render(width);
};
const base: Props = {
	tabs: [tab("D4 plain pi", true, false), tab("D5 key source", false, true), tab("D6 machine allow", false, false)],
	submit: { active: false, allAnswered: false },
};
const hooked = <T>(fn: () => T): T => {
	const off = installHint();
	try {
		return fn();
	} finally {
		off();
	}
};

test("the tab line is pills joined by ─ with no arrows, boxes or check marks", () => {
	console.log("today:", JSON.stringify(D(base)[0]));
	const line = hooked(() => D(base)[0]!);
	assert.equal(line, ` ${pill(GREEN, "D4 plain pi")}${SEP}${pill(BLUE, "D5 key source")}${SEP}${pill(GREY, "D6 machine allow")}${SEP}${pill(GREY, "Submit")}`);
	assert.doesNotMatch(line, /[←→■□✓]/);
});

test("a pill has caps, the colour inverted onto its text and no padding inside", () => {
	const line = hooked(() => D({ tabs: [tab("Scope", false, true)], submit: { active: false, allAnswered: false } })[0]!);
	assert.ok(line.includes(`${E}34m\uE0B6${E}7mScope${E}27m${E}34m\uE0B4${E}39m`), line);
	assert.doesNotMatch(line, /\x1b\[(2m|48;|38;)/);
	assert.doesNotMatch(line, /\x1b\[(30m|4)/);
	assert.doesNotMatch(line, / \x1b\[27m/);
});

test("the current question is blue, an answered one green and an unanswered one grey", () => {
	const line = hooked(() => D(base)[0]!);
	assert.ok(line.includes(pill(BLUE, "D5 key source")), "current: blue");
	assert.ok(line.includes(pill(GREEN, "D4 plain pi")), "answered: green");
	assert.ok(line.includes(pill(GREY, "D6 machine allow")), "unanswered: grey");
});

test("a current tab that is also answered is blue", () => {
	const props: Props = { tabs: [tab("D4", true, true), tab("D5", true, false)], submit: { active: false, allAnswered: false } };
	const line = hooked(() => D(props)[0]!);
	assert.ok(line.includes(pill(BLUE, "D4")));
	assert.ok(line.includes(pill(GREEN, "D5")));
	assert.ok(!line.includes(pill(GREEN, "D4")));
});

test("Submit is blue when current, green when every question is answered, else grey, and nothing follows it", () => {
	const tabs = [tab("D4", true, false)];
	const submit = (s: Props["submit"]) => hooked(() => D({ tabs, submit: s })[0]!);
	const current = submit({ active: true, allAnswered: true });
	const done = submit({ active: false, allAnswered: true });
	const open = submit({ active: false, allAnswered: false });
	assert.ok(current.endsWith(pill(BLUE, "Submit")), "current is blue, even when all answered");
	assert.ok(done.endsWith(pill(GREEN, "Submit")), "all answered is green");
	assert.ok(open.endsWith(pill(GREY, "Submit")), "otherwise grey");
});

test("only the separators are colour 8", () => {
	const line = hooked(() => D(base)[0]!);
	assert.equal(line.split(`${E}90m`).length - 1, 3, "one colour-8 span per separator");
	assert.equal(line.split(SEP).length - 1, 3, "three separators, each a single ─");
	assert.equal(line.replaceAll(SEP, "").includes(`${E}90m`), false);
	assert.doesNotMatch(line, /\x1b\[38;5;8m/);
});

test("a line wider than the pane is cut at the pane width", () => {
	const line = hooked(() => D(base, 20)[0]!);
	assert.ok(line.includes("\uE0B6"), "pills, not the old text");
	assert.equal(visibleWidth(line), 20);
	assert.ok(D(base, 20).length === 3, "one row for the bar, one empty, one for the other child: not wrapped");
});

test("the tab bar keeps its empty second row", () => {
	const rows = hooked(() => D(base));
	assert.equal(rows.length, 3);
	assert.ok(rows[0]!.includes("\uE0B6"), "row 0 is the pill line");
	assert.equal(rows[1], "");
	assert.equal(rows[2], " ← x →");
});

test("another component's line shaped like the old tab line is untouched", () => {
	const hookedRows = hooked(() => D(base));
	assert.equal(hookedRows[2], " ← x →", "a component of another name");
	assert.ok(hookedRows[0]!.includes("\uE0B6"), "while the TabBar beside it is rendered as pills");
	// a TabBar-named class whose props lack the tabs/submit shape is not recognised
	class Lookalike implements Component {
		props = { tabs: "no" };
		invalidate(): void {}
		render(): string[] {
			return [" ← ■ a ✓ Submit →", ""];
		}
	}
	Object.defineProperty(Lookalike, "name", { value: "TabBar" });
	const box = new Container();
	box.addChild(new Lookalike());
	assert.deepEqual(hooked(() => box.render(80)), [" ← ■ a ✓ Submit →", ""]);
});

test("the hook is idempotent and its removal restores pi-tui's own render", () => {
	const original = Container.prototype.render;
	const off1 = installHint();
	const wrapped = Container.prototype.render;
	const off2 = installHint();
	try {
		assert.notEqual(wrapped, original, "installing wraps Container.prototype.render");
		assert.equal(Container.prototype.render, wrapped, "a second install does not wrap again");
		assert.ok(D(base)[0]!.includes("\uE0B6"));
	} finally {
		off2();
		off1();
	}
	assert.equal(Container.prototype.render, original);
	assert.match(D(base)[0]!, /← /);
});
