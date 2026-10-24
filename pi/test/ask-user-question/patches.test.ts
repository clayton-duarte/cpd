/**
 * The copy's own TabBar and single-question header builder (pi/ask-user-question) pass through installHint: the grey
 * title pill and the tab line of ask-header.test.ts and ask-tabs.test.ts, here from the copy, not a reproduction.
 * The code under test is imported by a computed specifier so the typecheck passes before the copy exists.
 */
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { Container, type Component } from "@earendil-works/pi-tui";
import { installHint } from "../../lib/hint.ts";

const THEME = new URL("../../themes/dango.json", import.meta.url);
type ThemeLike = { fg(c: string, t: string): string; bg(c: string, t: string): string; bold(t: string): string };
const pi = fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent"));
const themeMod = (await import(pathToFileURL(join(dirname(pi), "modes/interactive/theme/theme.js")).href)) as {
	loadThemeFromPath(path: string, mode?: string): ThemeLike;
};
const theme = themeMod.loadThemeFromPath(THEME.pathname, "256color");
const E = "\x1b[";
const SEP = `${E}90m─${E}39m`;
const pill = (n: number, label: string): string =>
	`${E}3${n}m\uE0B6${E}7m${label}${E}27m${E}3${n}m\uE0B4${E}39m`;

type TabBarLike = Component & {
	setProps(p: {
		tabs: ReadonlyArray<{ label: string; answered: boolean; active: boolean }>;
		submit: { active: boolean; allAnswered: boolean };
	}): void;
};
type StrategyLike = { headingRows(state: { currentTab: number }): Component[] };

const TAB_BAR = new URL("../../ask-user-question/view/components/tab-bar.ts", import.meta.url);
const STRATEGY = new URL("../../ask-user-question/view/tab-content-strategy.ts", import.meta.url);

test("the copy TabBar and header builder pass through installHint", async () => {
	assert.ok(existsSync(TAB_BAR), "pi/ask-user-question/view/components/tab-bar.ts exists");
	assert.ok(existsSync(STRATEGY), "pi/ask-user-question/view/tab-content-strategy.ts exists");
	const { TabBar } = (await import(TAB_BAR.href)) as { TabBar: new (t: ThemeLike) => TabBarLike };
	const { QuestionTabStrategy } = (await import(STRATEGY.href)) as {
		QuestionTabStrategy: new (config: unknown) => StrategyLike;
	};

	const bar = new TabBar(theme);
	bar.setProps({
		tabs: [
			{ label: "D4 plain pi", answered: true, active: false },
			{ label: "D5 key source", answered: false, active: true },
			{ label: "D6 machine allow", answered: false, active: false },
		],
		submit: { active: false, allAnswered: false },
	});
	const other: Component = { invalidate() {}, render: () => [" ← x →"] };
	const box = new Container();
	box.addChild(bar);
	box.addChild(other);

	const strategy = new QuestionTabStrategy({
		questions: [{ header: "Assumptions", question: "Which?", options: [] }],
		tabsByIndex: [],
		theme,
		isMulti: false,
		getCurrentBodyHeight: () => 10,
		collapseKey: "off",
	});

	const off = installHint();
	try {
		const rows = box.render(120);
		assert.equal(
			rows[0],
			` ${pill(2, "D4 plain pi")}${SEP}${pill(4, "D5 key source")}${SEP}${pill(7, "D6 machine allow")}${SEP}${pill(7, "Submit")}`,
		);
		assert.doesNotMatch(rows[0]!, /[←→■□✓]/);
		assert.equal(rows[2], " ← x →", "another component is untouched");

		const heading = strategy.headingRows({ currentTab: 0 })[0]!.render(80)[0]!;
		assert.ok(heading.includes(pill(7, "Assumptions")), heading);
	} finally {
		off();
	}
});
