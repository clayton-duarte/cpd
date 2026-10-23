/** Reload after an install (docs/contract.md 5.3 iii): the newest load's behaviour runs for the tool-row patch and patchRender. */
import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";

const repo = new URL("../../../", import.meta.url).pathname;
const extensions = new URL("../../compact-tools/extensions/", import.meta.url).pathname;
const scratch = mkdtempSync(join(tmpdir(), "prototype-reload-"));
test.after(() => rmSync(scratch, { recursive: true, force: true }));

const globals = globalThis as Record<symbol | string, unknown>;
const RESOLVER_KEY = Symbol.for("pi-compact-tools.custom.resolver.v2");
const LEGACY_RESOLVER_KEY = Symbol.for("pi-compact-tools.custom.resolver");
const OVERRIDES_KEY = Symbol.for("pi-compact-tools.silent.overrides.v2");
const STATE_KEY = Symbol.for("pi-compact-tools.silent.state");
test.afterEach(() => {
	for (const key of [RESOLVER_KEY, LEGACY_RESOLVER_KEY, OVERRIDES_KEY, STATE_KEY]) delete globals[key];
	delete globals.__reloadLog;
});

/** A copy of the extension tree whose two observable spots carry `tag`; node_modules resolves to the repo's. */
type Custom = typeof import("../../compact-tools/extensions/compact-tools-custom.ts");
const trees: Promise<Custom>[] = [];
function tree(tag: string): Promise<Custom> {
	const root = mkdtempSync(join(scratch, `${tag}-`));
	symlinkSync(join(repo, "node_modules"), join(root, "node_modules"));
	cpSync(extensions, join(root, "ext"), { recursive: true });
	const edit = (file: string, from: string, to: string) => {
		const path = join(root, "ext", file);
		const source = readFileSync(path, "utf8");
		assert.ok(source.includes(from), `${file} no longer contains ${from}`);
		writeFileSync(path, source.replace(from, to));
	};
	edit("compact-tools-rendering.ts", "export function refreshRowShell(row: object): void {",
		`export function refreshRowShell(row: object): void {\n\t((globalThis as any).__reloadLog ??= []).push(${JSON.stringify(tag)});`);
	edit("compact-tools-custom.ts", 'resolve(this) ? "self" :', `resolve(this) ? ${JSON.stringify(`self-${tag}`)} :`);
	const loaded = import(pathToFileURL(join(root, "ext", "compact-tools-custom.ts")).href) as Promise<Custom>;
	trees.push(loaded);
	return loaded;
}

const renderers = { renderCall: () => "call", renderResult: () => "result" } as never;
function fakeRows() {
	class Rows {
		updateDisplay() { return "updated"; }
		getCallRenderer() { return "orig-call"; }
		getResultRenderer() { return "orig-result"; }
		getRenderShell() { return "orig-shell"; }
		hasRendererDefinition() { return false; }
	}
	return Rows.prototype;
}
const row = { toolName: "x" };

test("a single load of the tool-row patch routes as before", async () => {
	const one = await tree("A");
	const proto = fakeRows();
	one.setRowResolver(() => renderers);
	assert.equal(one.patchToolRows(proto), true);
	assert.equal((proto.getRenderShell as any).call(row), "self-A");
	assert.equal((proto.getCallRenderer as any).call(row), (renderers as any).renderCall);
	assert.equal((proto.updateDisplay as any).call(row), "updated");
	assert.deepEqual(globals.__reloadLog, ["A"]);
});

test("updateDisplay runs the newest load's refreshRowShell after a second load of the tool-row patch", async () => {
	const [one, two] = [await tree("A"), await tree("B")];
	const proto = fakeRows();
	one.patchToolRows(proto);
	two.patchToolRows(proto);
	(proto.updateDisplay as any).call({ toolName: "x" });
	assert.deepEqual(globals.__reloadLog, ["B"]);
});

test("the routed tool-row methods use the newest load's routing after a second load", async () => {
	const [one, two] = [await tree("A"), await tree("B")];
	const proto = fakeRows();
	one.setRowResolver(() => renderers);
	one.patchToolRows(proto);
	two.patchToolRows(proto);
	assert.equal((proto.getRenderShell as any).call({ toolName: "x" }), "self-B");
});

type Silent = typeof import("../../compact-tools/extensions/compact-tools-silent.ts");
const silentUrl = new URL("../../compact-tools/extensions/compact-tools-silent.ts", import.meta.url).href;
const silent = async (n: number) => (await import(`${silentUrl}?load=${n}`)) as Silent;
const fakeRender = () => (class { render() { return ["orig"]; } }).prototype;

test("a single load of patchRender engages and bypasses by its rule", async () => {
	const one = await silent(21);
	const engaged = fakeRender();
	one.patchRender(engaged, () => ["A"], () => true);
	assert.deepEqual(engaged.render(), ["A"]);
	const bypassed = fakeRender();
	one.patchRender(bypassed, () => ["A"], () => false);
	assert.deepEqual(bypassed.render(), ["orig"]);
});

test("the render override's engaged rule is the newest load's after a second load", async () => {
	const [one, two] = [await silent(31), await silent(32)];
	const proto = fakeRender();
	one.patchRender(proto, () => ["A"], () => true);
	two.patchRender(proto, () => ["B"], () => false);
	assert.deepEqual(proto.render(), ["orig"], "load 2 says not engaged");
	two.patchRender(proto, () => ["B"], () => true);
	assert.deepEqual(proto.render(), ["B"], "load 2 says engaged");
});
