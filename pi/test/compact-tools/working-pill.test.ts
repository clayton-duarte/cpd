/** The working label is a pill in 6 drawn by the extension's own clock; pi's Loader gets no frames (contract 6.1.1, 6.9). */
import "./pinned-themes.ts";
import assert from "node:assert/strict";
import test from "node:test";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ProgressController } from "../../compact-tools/extensions/compact-tools-progress.ts";
import { restoreClocks } from "./indicator-harness.ts";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

test.after(() => restoreClocks());

/** The working pill's caps. */
const LEFT_CAP = "\ue0b6";
const RIGHT_CAP = "\ue0b4";

type Handler = (event: unknown) => void;

function drive(run: (handlers: Map<string, Handler>) => void): { messages: string[]; indicator: { frames: string[]; intervalMs?: number } | undefined } {
	const handlers = new Map<string, Handler>();
	const pi = { on: (name: string, handler: Handler) => handlers.set(name, handler), getThinkingLevel: () => "medium" } as unknown as ExtensionAPI;
	const messages: string[] = [];
	let indicator: { frames: string[]; intervalMs?: number } | undefined;
	const controller = new ProgressController(pi);
	controller.bind({
		isIdle: () => false,
		ui: {
			theme,
			setWorkingVisible: () => {},
			setWorkingIndicator: (value?: typeof indicator) => { if (value) indicator = value; },
			setWorkingMessage: (message?: string) => { if (message) messages.push(message); },
		},
	} as unknown as ExtensionContext);
	try { run(handlers); } finally { controller.dispose(); }
	return { messages, indicator };
}

const strip = (value: string) => value.replace(/\x1b\[[0-9;]*m/gu, "");

const LABELS: Array<[string, (handlers: Map<string, Handler>) => void]> = [
	["Thinking…", (h) => h.get("agent_start")!({})],
	["Responding…", (h) => h.get("message_update")!({ assistantMessageEvent: { type: "text_delta" } })],
	["Preparing tools…", (h) => h.get("message_update")!({ assistantMessageEvent: { type: "toolcall_delta" } })],
	["Running command…", (h) => h.get("tool_execution_start")!({ toolCallId: "1", toolName: "bash", args: { command: "ls" } })],
	["Processing results…", (h) => { h.get("tool_execution_start")!({ toolCallId: "1", toolName: "bash", args: {} }); h.get("tool_execution_end")!({ toolCallId: "1" }); }],
];

test("the working label is a pill with caps 6 and its text inverted", () => {
	const { messages } = drive((h) => h.get("agent_start")!({}));
	const message = messages.at(-1)!;
	assert.ok(message, "a label is set");
	assert.ok(message.includes(LEFT_CAP) && message.includes(RIGHT_CAP), "both caps are drawn");
	assert.match(message, /\x1b\[36m\ue0b6/u, "the left cap is 6 on the terminal background");
	assert.match(message, /\x1b\[36m\ue0b4/u, "the right cap is 6 on the terminal background");
	assert.ok(message.includes("\x1b[7m") && message.includes("\x1b[27m"), "the text is inverted");
	assert.ok(!message.includes("\x1b[46m") && !message.includes("\x1b[30m"), "no bg 6, no text 0");
	assert.ok(strip(message).startsWith(LEFT_CAP) && strip(message).endsWith(RIGHT_CAP));
	const inside = strip(message).slice(1, -1);
	assert.equal(inside, inside.trim(), "no padding space inside the caps (hs-d475)");
});

test("every working label keeps its words and ends with …", () => {
	for (const [label, act] of LABELS) {
		const { messages } = drive(act);
		const message = messages.at(-1);
		assert.ok(message, `${label} is set`);
		const plain = strip(message!).replace(LEFT_CAP, "").replace(RIGHT_CAP, "");
		assert.equal(plain, label, "the words sit unpadded between the caps");
		assert.ok(plain.endsWith("…") && !plain.includes("..."));
		assert.ok(message!.includes(LEFT_CAP), `${label} is a pill`);
	}
});

test("the working indicator carries no frames for pi's Loader to animate", () => {
	const { indicator } = drive((h) => h.get("agent_start")!({}));
	assert.ok(indicator, "an indicator is set");
	assert.deepEqual(indicator!.frames, [], "the empty frame list leaves pi's Loader without an interval");
	assert.equal(indicator!.intervalMs, undefined, "no interval is named either");
});

test("the label pill uses only indexed SGR 0-7", () => {
	for (const [, act] of LABELS) {
		const { messages } = drive(act);
		const message = messages.at(-1)!;
		assert.ok(message.includes(LEFT_CAP), "it is a pill");
		for (const match of message.matchAll(/\x1b\[([0-9;]*)m/gu)) {
			const params = match[1]!.split(";").map(Number);
			const [first] = params;
			const allowed = (first! >= 30 && first! <= 37) || (first! >= 40 && first! <= 47) || first === 39 || first === 49 || first === 0 || first === 22 || first === 23 || first === 3 || first === 24 || first === 27 || first === 7;
			const indexed = (first === 38 || first === 48) && params[1] === 5 && params[2]! < 8;
			assert.ok(allowed || indexed, `SGR ${match[1]} is outside the indexed 0-7 set`);
			assert.ok(![2].includes(first!), `SGR ${first} is forbidden`);
		}
	}
});

test("a label that changes from Thinking to Responding stays a pill", () => {
	const { messages } = drive((h) => {
		h.get("agent_start")!({});
		h.get("message_update")!({ assistantMessageEvent: { type: "text_delta" } });
	});
	const thinking = messages.find((m) => strip(m).includes("Thinking…"));
	const responding = messages.at(-1)!;
	assert.ok(thinking, "Thinking is set first");
	assert.ok(strip(responding).includes("Responding…"));
	for (const message of [thinking!, responding]) {
		assert.ok(message.includes(LEFT_CAP) && message.includes(RIGHT_CAP), "caps");
		assert.ok(message.includes("\x1b[7m") && !message.includes("\x1b[30m"), "inverted text");
	}
});

/** A controller whose label is observed as set (string) or cleared (undefined); `idle` and the events are the test's. */
function compaction(idle: boolean) {
	const handlers = new Map<string, Handler>();
	const pi = { on: (name: string, handler: Handler) => handlers.set(name, handler), getThinkingLevel: () => "medium" } as unknown as ExtensionAPI;
	let label: string | undefined;
	const controller = new ProgressController(pi);
	controller.bind({
		isIdle: () => idle,
		ui: {
			theme,
			setWorkingVisible: () => {},
			setWorkingIndicator: () => {},
			setWorkingMessage: (message?: string) => { label = message; },
		},
	} as unknown as ExtensionContext);
	return {
		emit: (name: string, event: unknown = {}) => handlers.get(name)?.(event),
		label: () => (label === undefined ? undefined : strip(label).replace(LEFT_CAP, "").replace(RIGHT_CAP, "")),
		dispose: () => controller.dispose(),
	};
}

const START_BASH = { toolCallId: "1", toolName: "bash", args: { command: "ls" } };

test("session_before_compact sets the working label to Compacting", () => {
	const c = compaction(false);
	try {
		c.emit("session_before_compact");
		assert.equal(c.label(), "Compacting…");
	} finally { c.dispose(); }
});

test("session_compact clears the working label when the agent is idle and no tool is running", () => {
	const c = compaction(true);
	try {
		c.emit("session_before_compact");
		assert.equal(c.label(), "Compacting…");
		c.emit("session_compact");
		assert.equal(c.label(), undefined);
	} finally { c.dispose(); }
});

test("session_compact restores the last running tool label when a tool is running", () => {
	const c = compaction(false);
	try {
		c.emit("tool_execution_start", START_BASH);
		c.emit("session_before_compact");
		assert.equal(c.label(), "Compacting…");
		c.emit("session_compact");
		assert.equal(c.label(), "Running command…");
	} finally { c.dispose(); }
});

test("session_compact_failed clears the working label when the agent is idle and no tool is running", () => {
	const c = compaction(true);
	try {
		c.emit("session_before_compact");
		assert.equal(c.label(), "Compacting…");
		c.emit("session_compact_failed");
		assert.equal(c.label(), undefined);
	} finally { c.dispose(); }
});

test("session_compact_failed restores the last running tool label when a tool is running", () => {
	const c = compaction(false);
	try {
		c.emit("tool_execution_start", START_BASH);
		c.emit("session_before_compact");
		assert.equal(c.label(), "Compacting…");
		c.emit("session_compact_failed");
		assert.equal(c.label(), "Running command…");
	} finally { c.dispose(); }
});

test("session_compact restores the Thinking label when the agent is busy and no tool is running", () => {
	const c = compaction(false);
	try {
		c.emit("session_before_compact");
		c.emit("session_compact");
		assert.equal(c.label(), "Thinking…");
	} finally { c.dispose(); }
});
