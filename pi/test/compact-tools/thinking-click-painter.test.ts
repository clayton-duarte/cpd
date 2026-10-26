/**
 * Contract 6.9 "The Thinking fold", R2: a left click expands a folded Thinking block and folds an expanded one,
 * also under cpd's mentions painter (a Lead's or Manager's pane), where the assistant override swaps children.
 */
import "./indicator-harness.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { theme } from "../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { AssistantMessageComponent, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ThinkingCycleController } from "../../compact-tools/extensions/compact-tools-thinking.ts";
import { loadExtension, shutdown } from "./indicator-harness.ts";

const SGR = /\x1b\[[0-9;]*m/gu;
const OSC = /\x1b\][^\x07]*\x07/gu;
const bare = (row: string) => row.replace(OSC, "").replace(SGR, "");
const DETAIL = "the detail text of the thought";
const WIDTH = 80;

const key = Symbol.for("cpd.mentions.painter");
const holder = globalThis as unknown as Record<symbol, ((line: string) => string) | undefined>;

const CASES = [
	{ name: "painter set, click on the title row", painter: true, y: 1 },
	{ name: "painter set, click on the toggle row", painter: true, y: 2 },
	{ name: "guard: painter not set, click on the title row", painter: false, y: 1 },
];

test("a left click on a Thinking block expands and folds it, with cpd's mentions painter set or not", async () => {
	for (const c of CASES) {
		if (c.painter) holder[key] = (line) => line;
		const harness = await loadExtension({}, { tui: true });
		let controller: ThinkingCycleController | undefined;
		try {
			let transform: ((markdown: string, context: unknown) => string) | undefined;
			const pi = { on: () => {}, registerMarkdownTransformer: (h: typeof transform) => { transform = h; } } as unknown as ExtensionAPI;
			controller = new ThinkingCycleController(pi, () => true, () => ({ expand: "\u{F0B26}", collapse: "\u{F0B2C}" }));
			const message = new AssistantMessageComponent({
				role: "assistant", stopReason: "stop", timestamp: Date.parse("2026-10-14T21:41:02.114Z"),
				content: [{ type: "thinking", thinking: `${DETAIL}\n\nmore` }, { type: "text", text: "done" }],
			} as never, true, undefined, undefined, 1, [(m: string, x: unknown) => transform!(m, x)] as never);
			controller.bind({ ui: { theme, setHiddenThinkingLabel: (l?: string) => { if (l !== undefined) message.setHiddenThinkingLabel(l); }, onTerminalInput: () => () => {} } } as unknown as ExtensionContext);
			const click = () => {
				const lines = message.render(WIDTH);
				(message as any).handleMouse?.({ type: "click", button: "left", x: 3, y: c.y, width: WIDTH, height: lines.length });
			};
			const shown = () => message.render(WIDTH).map(bare).join("\n").includes(DETAIL);
			assert.equal(shown(), false, `${c.name}: folded at first`);
			click();
			assert.equal(shown(), true, `${c.name}: the first click expands`);
			click();
			assert.equal(shown(), false, `${c.name}: the second click folds`);
		} finally {
			controller?.dispose();
			shutdown(harness);
			delete holder[key];
		}
	}
});
