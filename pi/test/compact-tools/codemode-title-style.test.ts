import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { stripTerminalSequences } from "@earendil-works/pi-tui";
import { loadThemeFromPath } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { NestedCallSummaries } from "../../compact-tools/extensions/compact-tools-nested.ts";
import { chrome, ink } from "../../compact-tools/extensions/compact-tools-indices.ts";

const nestedTitle = (text: string) => ink("toolTitle", text);
const count = (color: "success" | "error" | "warning") => (text: string) => ink(color, text);

for (const mode of ["truecolor", "256color"] as const) test(`codemode titles and counts follow github-dark-pro and light palettes (${mode})`, () => {
	const cache = new NestedCallSummaries(); cache.clear(); cache.begin("styled", "codemode");
	for (const [index, name] of ["read", "read", "bash", "edit"].entries()) {
		cache.note({ toolCallId: `styled/${index}`, parentToolCallId: "styled", toolName: name }, false);
		cache.note({ toolCallId: `styled/${index}`, parentToolCallId: "styled", toolName: name, isError: false,
			result: name === "edit" ? { content: [{ type: "text", text: "edited" }], details: { diff: "- 1 old\n+ 1 new\n+ 2 added" } } : undefined }, true);
	}
	for (const path of ["./fixtures/github-dark-pro.json", "./fixtures/theme-light.json", "./fixtures/github-dark-pro.json"]) {
		const theme = loadThemeFromPath(fileURLToPath(new URL(path, import.meta.url)), mode);
		const result = cache.format("styled", theme)!;
		assert.equal(stripTerminalSequences(result), "read 2 · bash 1 · edit 1");
		for (const name of ["read", "bash", "edit"]) assert.ok(result.includes(nestedTitle(name)), `${name} is title text without bold`);
		assert.ok(result.includes(count("success")("2")), "successful counts use success color");
		assert.ok(result.includes(nestedTitle("read") + chrome(" ") + count("success")("2")), "name and count are separated by a single chrome-colored space");
		assert.ok(stripTerminalSequences(result).includes("edit 1"), "singleton outcomes remain explicit");
		assert.doesNotMatch(stripTerminalSequences(result), /\+2|-1/u, "edit diff totals are not displayed");
		if (path.includes("github-dark-pro") && mode === "truecolor") {
			assert.ok(result.includes("\x1b[37m"), "the title is an indexed colour, not the theme's RGB");
			assert.doesNotMatch(result, /38;2/u, "no RGB is drawn");
			assert.notEqual(nestedTitle("read"), chrome(theme.bold("read")), "title hue is not replaced by gray chrome");
			assert.ok(!result.includes("\x1b[1m"), "summary names do not use bold");
			assert.equal(theme.getFgAnsi("toolTitle"), "\x1b[38;2;139;133;214m", "ordinary tool titles are unchanged");
		}
	}
	cache.clear();
});
