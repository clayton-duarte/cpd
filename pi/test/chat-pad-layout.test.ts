/**
 * Chat pad layout (docs/contract.md section 6.1 "chat pad"): the pad's layout does not depend on how the root is
 * handed to pi-tui. The padded root is compared with the plain nested stack (pad rows around an `HStack` of the root)
 * at an even and an odd width: every layout rect, and where a wheel, a click and a press in a pad column are routed.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { Container, HStack, ScrollView, Spacer, VStack, type Component } from "@earendil-works/pi-tui";
import { getLayoutBoxesAt, getScrollViewsAt, renderLayoutFrame } from "@earendil-works/pi-tui/dist/layout.js";
import { dispatchMouseEvent, type MouseEvent } from "@earendil-works/pi-tui/dist/tui.js";
import { paddedRoot } from "../lib/pad.ts";

const HEIGHT = 12;

class Text implements Component {
	private readonly text: string;
	constructor(text: string) { this.text = text; }
	render(width: number): string[] { return [this.text.slice(0, width).padEnd(width)]; }
	invalidate(): void {}
}

class Clickable extends Text {
	events: string[] = [];
	handleMouse(event: { type: string }): { handled: true } { this.events.push(event.type); return { handled: true }; }
}

function build() {
	const document = new Container();
	for (let i = 0; i < 40; i++) document.addChild(new Text(`message ${i}`));
	const transcript = new ScrollView(document, { follow: "end", primary: true });
	const editor = new Clickable("> editor");
	const dock = new Text("dock");
	const root = new VStack([
		{ component: transcript, basis: 0, grow: 1, shrink: 1, minSize: 1 },
		{ component: dock, basis: "auto", grow: 0, shrink: 1, minSize: 1 },
		{ component: editor, basis: "auto", grow: 0, shrink: 1, minSize: 1 },
	]);
	return { root, transcript, editor };
}

const padRow = () => ({ component: new Spacer(1), basis: 1, minSize: 1, maxSize: 1 });

/** The nested stack the pad was before it measured nothing: no basis on the root or on its HStack. */
const plainPad = (root: Component): Component =>
	new VStack([
		padRow(),
		{ component: new HStack([padRow(), { component: root, grow: 1, shrink: 1, minSize: 1 }, padRow()]), grow: 1, shrink: 1, minSize: 1 },
		padRow(),
	]);

type Box = ReturnType<typeof renderLayoutFrame>["root"];

const rects = (box: Box): string[] => [
	JSON.stringify([box.rect, box.clip, Boolean(box.scrollView)]),
	...box.children.flatMap((child: Box) => rects(child)),
];

/** pi-tui's `dispatchMouseToLayout`, which `TuiAltScreen` runs for a mouse event: the first box with a handler. */
function route(layout: ReturnType<typeof renderLayoutFrame>, type: string, x: number, y: number, width: number): string[] {
	const reached: string[] = [];
	for (const box of getLayoutBoxesAt(layout, x, y) as Box[]) {
		if (box.component instanceof Container && box.component.handleMouse === Container.prototype.handleMouse) continue;
		const event = { type, screenX: x, screenY: y, x: x - box.rect.x, y: y - box.rect.y, width: box.rect.width, height: box.rect.height, button: 0 } as unknown as MouseEvent;
		if (dispatchMouseEvent(box.component, event)) {
			reached.push(box.component.constructor.name);
			break;
		}
	}
	return reached;
}

for (const width of [80, 81]) {
	test(`chat pad layout: the padded root lays out and routes the mouse as the plain pad does at width ${width}`, () => {
		const plain = build();
		const padded = build();
		const a = renderLayoutFrame(plainPad(plain.root), width, HEIGHT, () => {});
		const b = renderLayoutFrame(paddedRoot(padded.root), width, HEIGHT, () => {});
		assert.deepEqual(b.lines, a.lines);
		assert.deepEqual(rects(b.root), rects(a.root));
		assert.ok(rects(b.root).length > 5);

		const editorRow = HEIGHT - 2;
		for (const [type, x, y] of [["wheel", 10, 3], ["click", 10, 3], ["click", 10, editorRow], ["click", 0, 3], ["click", width - 1, 3], ["click", 10, 0]] as const) {
			assert.deepEqual(route(b, type, x, y, width), route(a, type, x, y, width), `${type} at ${x},${y}`);
		}
		assert.deepEqual(getScrollViewsAt(a, 10, 3), [plain.transcript]);
		assert.deepEqual(route(b, "click", 10, editorRow), ["Clickable"], "a click on the editor row reaches the editor");
		assert.deepEqual(route(b, "click", 0, 3), [], "a click in the left pad column reaches nothing");
		assert.deepEqual(route(b, "click", width - 1, 3), [], "a click in the right pad column reaches nothing");
		// pi-tui scrolls the scroll views under the pointer for a wheel (`getScrollViewsAt`), not through `handleMouse`.
		assert.deepEqual(getScrollViewsAt(b, 10, 3), [padded.transcript], "a wheel on the transcript reaches the ScrollView");
		assert.deepEqual(getScrollViewsAt(b, 0, 3), [], "a wheel in a pad column reaches no ScrollView");
		assert.deepEqual(getScrollViewsAt(a, 0, 3), [], "as in the plain pad");
	});
}
