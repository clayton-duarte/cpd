import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { Theme, type ExtensionAPI } from "@earendil-works/pi-coding-agent";
import compactTools from "../../compact-tools/extensions/compact-tools.ts";
import { hookItalicSpans, ITALIC_TOKENS, italicFg } from "../../compact-tools/extensions/compact-tools-italic.ts";

/** contract 6.1.1 (pro-d38): under the theme dango, four Theme.fg tokens come back in SGR 3 around pi's own output. */
const original = function (this: unknown, ...args: unknown[]): string { return `\x1b[36m${String(args[1])}\x1b[39m`; };
const dango = { name: "dango" };
const OTHERS = ["thinkingMedium", "accent", "mdCodeBlock", "syntaxKeyword", "toolTitle", "error"];

test("italic spans: mdCode, syntaxFunction, syntaxType and customMessageLabel come back in SGR 3 under dango", () => {
	assert.deepEqual([...ITALIC_TOKENS], ["mdCode", "syntaxFunction", "syntaxType", "customMessageLabel"]);
	for (const token of ITALIC_TOKENS) assert.equal(italicFg(dango, [token, "text"], original), "\x1b[3m\x1b[36mtext\x1b[39m\x1b[23m", token);
});

test("italic spans: every other token comes back as pi drew it under dango", () => {
	for (const token of OTHERS) assert.equal(italicFg(dango, [token, "text"], original), "\x1b[36mtext\x1b[39m", token);
});

test("italic spans: no token is changed under another theme", () => {
	for (const token of [...ITALIC_TOKENS, ...OTHERS]) assert.equal(italicFg({ name: "dark" }, [token, "text"], original), "\x1b[36mtext\x1b[39m", token);
});

const fakeTheme = () => ({ name: "dango", fg(color: string, text: string) { return `\x1b[36m${text}\x1b[39m${color === "mdCode" ? "" : ""}`; } });

test("italic spans: nothing is changed while the extension is off", () => {
	const proto = fakeTheme();
	hookItalicSpans(() => false, proto);
	assert.equal(proto.fg("mdCode", "text"), "\x1b[36mtext\x1b[39m");
	const on = fakeTheme();
	hookItalicSpans(() => true, on);
	assert.equal(on.fg("mdCode", "text"), "\x1b[3m\x1b[36mtext\x1b[39m\x1b[23m");
});

test("italic spans: hooking again replaces the rule and never stacks italic", () => {
	const proto = fakeTheme();
	hookItalicSpans(() => true, proto);
	hookItalicSpans(() => true, proto);
	assert.equal(proto.fg("mdCode", "text"), "\x1b[3m\x1b[36mtext\x1b[39m\x1b[23m");
	hookItalicSpans(() => false, proto);
	assert.equal(proto.fg("mdCode", "text"), "\x1b[36mtext\x1b[39m", "the second call's enabled rules");
});

test("italic spans: compact-tools installs the hook on Theme.fg at load", () => {
	const dir = mkdtempSync(join(tmpdir(), "compact-italic-"));
	const previous = process.env.PI_CODING_AGENT_DIR;
	process.env.PI_CODING_AGENT_DIR = dir;
	try {
		const pi = new Proxy({} as Record<string, unknown>, { get: (_t, method: string) => (method === "getThinkingLevel" ? () => "medium" : () => {}) }) as unknown as ExtensionAPI;
		compactTools(pi);
		assert.ok(Object.hasOwn(Theme.prototype, Symbol.for("pi-compact-tools.hook.theme.fg.italic")));
	} finally {
		if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR; else process.env.PI_CODING_AGENT_DIR = previous;
		rmSync(dir, { recursive: true, force: true });
	}
});
