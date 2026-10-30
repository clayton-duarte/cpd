/**
 * A double load (docs/contract.md section 6.9, docs/implementing.md "The compact-tools extension" point 2): while the settings
 * still list the npm package pi-compact-tools, or a git source of that name, cpd's own copy stands down at load, before it
 * registers anything, and says so once per process. `otherCopy` is a pure function of the two `packages` lists.
 */
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { mock } from "node:test";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { findOtherCopy, otherCopy } from "../../compact-tools/extensions/compact-tools-double-load.ts";
import compactTools from "../../compact-tools/extensions/compact-tools.ts";

// --- otherCopy: a pure function of the two lists ----------------------------------------------

test("otherCopy finds the npm package, with or without a version, as a string or as an object's source", () => {
	for (const source of ["npm:pi-compact-tools@0.16.6", "npm:pi-compact-tools"]) {
		assert.deepEqual(otherCopy({ global: [source], project: [] }), { source, scope: "global" }, source);
		assert.deepEqual(otherCopy({ global: [{ source }], project: [] }), { source, scope: "global" }, `{source: ${source}}`);
	}
});

test("otherCopy finds a git source whose repository is pi-compact-tools, whoever owns it, with a ref or .git", () => {
	for (const source of [
		"git:github.com/clayton-duarte/pi-compact-tools@2c1fcad",
		"https://github.com/nedleeds/pi-compact-tools.git",
		"git:github.com/nedleeds/pi-compact-tools",
	]) assert.deepEqual(otherCopy({ global: [source], project: [] }), { source, scope: "global" }, source);
});

test("otherCopy answers the first entry of the global list, then of the project list, with its scope", () => {
	assert.deepEqual(otherCopy({ global: [], project: ["npm:pi-compact-tools@0.16.6"] }), { source: "npm:pi-compact-tools@0.16.6", scope: "project" });
	assert.deepEqual(
		otherCopy({ global: ["npm:@plannotator/pi-extension@0.28.17", "npm:pi-compact-tools@0.16.6"], project: ["npm:pi-compact-tools"] }),
		{ source: "npm:pi-compact-tools@0.16.6", scope: "global" },
	);
});

test("otherCopy does not find another package, a scoped one of the same name, a local path, or no packages", () => {
	for (const source of ["npm:@plannotator/pi-extension@0.28.17", "npm:pi-compact-tools-extra", "npm:@scope/pi-compact-tools", "/home/me/pi-compact-tools", "./pi-compact-tools"]) {
		assert.equal(otherCopy({ global: [source], project: [] }), undefined, source);
		assert.equal(otherCopy({ global: [], project: [{ source }] }), undefined, source);
	}
	assert.equal(otherCopy({ global: [], project: [] }), undefined);
});

// --- the factory over a fake pi ---------------------------------------------------------------

const FLAG = Symbol.for("cpd.compact-tools.double-load-said");
const ENV = "PI_CODING_AGENT_DIR";

type Call = { method: string; args: unknown[] };
type Handler = (event: unknown, ctx: unknown) => void;

/** A pi that records every registration the extension makes, and the handlers it hooks. */
function fakePi() {
	const calls: Call[] = [];
	const handlers = new Map<string, Handler[]>();
	const record = (method: string) => (...args: unknown[]) => { calls.push({ method, args }); };
	const pi = new Proxy({} as Record<string, unknown>, {
		get: (_target, method: string) => {
			if (method === "on") return (name: string, handler: Handler) => {
				calls.push({ method, args: [name, handler] });
				handlers.set(name, [...(handlers.get(name) ?? []), handler]);
			};
			if (method === "getThinkingLevel") return () => "medium";
			return record(method);
		},
	}) as unknown as ExtensionAPI;
	return { pi, calls, handlers };
}

/** A pi's `session_start` context in `mode`, with the `notify` calls recorded. */
function context(mode: string) {
	const notices: Array<{ message: string; level: string | undefined }> = [];
	const ctx = {
		cwd: process.cwd(), mode, isProjectTrusted: () => false,
		ui: new Proxy({ notify: (message: string, level?: string) => { notices.push({ message, level }); } } as Record<string, unknown>, {
			get: (target, key: string) => target[key] ?? (() => () => {}),
		}),
	} as unknown as ExtensionContext;
	return { ctx, notices };
}

/** The agent directory with `settings.json` set to `packages` (none: no file), and the said-flag cleared. */
function settings<T>(packages: unknown[] | undefined, fn: (agentDir: string) => T): T {
	const dir = mkdtempSync(join(tmpdir(), "compact-tools-double-"));
	if (packages !== undefined) writeFileSync(join(dir, "settings.json"), JSON.stringify({ packages }));
	const previous = process.env[ENV];
	process.env[ENV] = dir;
	delete (globalThis as Record<symbol, unknown>)[FLAG];
	try { return fn(dir); } finally {
		if (previous === undefined) delete process.env[ENV]; else process.env[ENV] = previous;
		delete (globalThis as Record<symbol, unknown>)[FLAG];
		rmSync(dir, { recursive: true, force: true });
	}
}

const LINE = (source: string, file: string) =>
	`cpd compact-tools: ${source} is also loaded (${file}): cpd's own copy stands down and registers nothing. Remove it from packages in ${file} and run /reload.`;

test("with another copy in the global settings the factory registers no tool, command or hook but one session_start handler", () => {
	settings(["npm:pi-compact-tools@0.16.6"], () => {
		const { pi, calls } = fakePi();
		compactTools(pi);
		assert.deepEqual(calls.map((call) => call.method), ["on"]);
		assert.equal(calls[0]!.args[0], "session_start");
	});
});

test("that handler notifies once, in tui mode, with the exact line and the warning level", () => {
	settings(["npm:pi-compact-tools@0.16.6"], (agentDir) => {
		const first = fakePi();
		compactTools(first.pi);
		const { ctx, notices } = context("tui");
		first.handlers.get("session_start")!.forEach((handler) => handler({ reason: "startup" }, ctx));
		assert.deepEqual(notices, [{ message: LINE("npm:pi-compact-tools@0.16.6", join(agentDir, "settings.json")), level: "warning" }]);
		// A second load in the same process (a /reload evaluates the factory anew) does not say it again.
		const second = fakePi();
		compactTools(second.pi);
		second.handlers.get("session_start")!.forEach((handler) => handler({ reason: "reload" }, ctx));
		assert.equal(notices.length, 1, "once per process");
		assert.equal((globalThis as Record<symbol, unknown>)[FLAG], true, "the flag is on globalThis under Symbol.for(\"cpd.compact-tools.double-load-said\")");
	});
});

test("in another mode it is one line on stderr, once", () => {
	settings([{ source: "git:github.com/clayton-duarte/pi-compact-tools@2c1fcad" }], (agentDir) => {
		const written: string[] = [];
		const stderr = mock.method(process.stderr, "write", (chunk: unknown) => { written.push(String(chunk)); return true; });
		try {
			for (const reason of ["startup", "reload"]) {
				const { pi, handlers } = fakePi();
				compactTools(pi);
				const { ctx, notices } = context("print");
				handlers.get("session_start")!.forEach((handler) => handler({ reason }, ctx));
				assert.deepEqual(notices, [], "print mode has no notification");
			}
		} finally { stderr.mock.restore(); }
		const lines = written.join("").split("\n").filter((line) => line.includes("cpd compact-tools:"));
		assert.deepEqual(lines, [LINE("git:github.com/clayton-duarte/pi-compact-tools@2c1fcad", join(agentDir, "settings.json"))]);
	});
});

test("another copy in the project's settings is named with the project's settings file", () => {
	settings([], () => {
		const project = mkdtempSync(join(tmpdir(), "compact-tools-double-project-"));
		mkdirSync(join(project, ".pi"));
		writeFileSync(join(project, ".pi", "settings.json"), JSON.stringify({ packages: ["npm:pi-compact-tools"] }));
		const previous = process.cwd();
		process.chdir(project);
		try {
			const { pi, calls, handlers } = fakePi();
			compactTools(pi);
			assert.deepEqual(calls.map((call) => call.method), ["on"]);
			const { ctx, notices } = context("tui");
			handlers.get("session_start")!.forEach((handler) => handler({ reason: "startup" }, ctx));
			assert.deepEqual(notices.map((notice) => notice.message), [LINE("npm:pi-compact-tools", join(process.cwd(), ".pi", "settings.json"))]);
		} finally { process.chdir(previous); rmSync(project, { recursive: true, force: true }); }
	});
});

test("with no other copy, or with other packages, the factory registers what it does today and says nothing", () => {
	for (const packages of [undefined, [], ["npm:@plannotator/pi-extension@0.28.17", "npm:pi-compact-tools-extra"]]) {
		settings(packages, () => {
			const { pi, calls, handlers } = fakePi();
			compactTools(pi);
			const methods = new Set(calls.map((call) => call.method));
			// The extension draws pi's tools through registerToolRenderer; it registers no tool of its own.
			assert.ok(methods.has("registerToolRenderer"), JSON.stringify(packages));
			assert.ok(methods.has("registerCommand"), "the /compact-tools command");
			assert.ok(handlers.size > 1, "its hooks");
			const { ctx, notices } = context("tui");
			handlers.get("session_start")?.forEach((handler) => handler({ reason: "startup" }, ctx));
			assert.deepEqual(notices.filter((notice) => notice.message.includes("cpd compact-tools:")), []);
			handlers.get("session_shutdown")?.forEach((handler) => handler({ reason: "quit" }, ctx));
		});
	}
});

test("findOtherCopy finds the other copy while pi holds the lock on the settings files", (t) => {
	const previous = process.env.PI_CODING_AGENT_DIR;
	const made = (prefix: string) => {
		const dir = mkdtempSync(join(tmpdir(), prefix));
		t.after(() => rmSync(dir, { recursive: true, force: true }));
		return dir;
	};
	const held = (file: string, packages: string[]) => {
		writeFileSync(file, JSON.stringify({ packages }));
		mkdirSync(`${file}.lock`);
	};
	t.after(() => { if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR; else process.env.PI_CODING_AGENT_DIR = previous; });
	const agent = made("compact-double-lock-agent-");
	held(join(agent, "settings.json"), ["npm:pi-compact-tools"]);
	process.env.PI_CODING_AGENT_DIR = agent;
	assert.deepEqual(findOtherCopy(made("compact-double-lock-cwd-"))?.copy, { source: "npm:pi-compact-tools", scope: "global" });
	const bare = made("compact-double-lock-bare-");
	writeFileSync(join(bare, "settings.json"), JSON.stringify({ packages: [] }));
	process.env.PI_CODING_AGENT_DIR = bare;
	const cwd = made("compact-double-lock-project-");
	mkdirSync(join(cwd, ".pi"));
	held(join(cwd, ".pi", "settings.json"), ["npm:pi-compact-tools"]);
	assert.deepEqual(findOtherCopy(cwd)?.copy, { source: "npm:pi-compact-tools", scope: "project" });
});
