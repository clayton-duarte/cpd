/** A double load of the npm package @juicesharp/rpiv-ask-user-question: cpd's copy stands down and says so once per process. */
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { mock } from "node:test";

const DOUBLE = new URL("../../ask-user-question/double-load.ts", import.meta.url);
const INDEX = new URL("../../ask-user-question/index.ts", import.meta.url);
const FLAG = Symbol.for("cpd.ask-user-question.double-load-said");
const NAME = "npm:@juicesharp/rpiv-ask-user-question";

type Source = string | { source: string };
type Found = { source: string; scope: "global" | "project" } | undefined;
type Lists = { global: readonly Source[]; project: readonly Source[] };

async function otherCopy(lists: Lists): Promise<Found> {
	assert.ok(existsSync(DOUBLE), "pi/ask-user-question/double-load.ts exists");
	return (await import(DOUBLE.href)).otherCopy(lists);
}

test("otherCopy finds the npm package, with or without a version, as a string or an object source", async () => {
	for (const source of [`${NAME}@2.13.8`, NAME]) {
		assert.deepEqual(await otherCopy({ global: [source], project: [] }), { source, scope: "global" }, source);
		assert.deepEqual(await otherCopy({ global: [{ source }], project: [] }), { source, scope: "global" }, `{source: ${source}}`);
	}
});

test("otherCopy answers the first entry of the global list, then of the project list, with its scope", async () => {
	assert.deepEqual(await otherCopy({ global: [], project: [`${NAME}@2.13.8`] }), { source: `${NAME}@2.13.8`, scope: "project" });
	assert.deepEqual(
		await otherCopy({ global: ["npm:@plannotator/pi-extension@0.28.17", `${NAME}@2.13.8`], project: [NAME] }),
		{ source: `${NAME}@2.13.8`, scope: "global" },
	);
});

test("otherCopy does not find another package, a git source, a local path, or no packages", async () => {
	for (const source of ["npm:@juicesharp/rpiv-config", "npm:@scope/other", "/home/me/rpiv-ask-user-question", "./rpiv-ask-user-question", "git:github.com/juicesharp/rpiv-mono@afc21bc"]) {
		assert.equal(await otherCopy({ global: [source], project: [] }), undefined, source);
		assert.equal(await otherCopy({ global: [], project: [{ source }] }), undefined, source);
	}
	assert.equal(await otherCopy({ global: [], project: [] }), undefined);
});

type Handler = (event: unknown, ctx: unknown) => void;

test("a found copy registers only session_start and says so once per process", async () => {
	assert.ok(existsSync(INDEX), "pi/ask-user-question/index.ts exists");
	const dir = mkdtempSync(join(tmpdir(), "ask-double-"));
	const keys = ["PI_CODING_AGENT_DIR", "CPD_SESSION", "CPD_PLAN", "CPD_ATTEMPT"];
	const saved = keys.map((key) => [key, process.env[key]] as const);
	writeFileSync(join(dir, "settings.json"), JSON.stringify({ packages: [`${NAME}@2.13.8`] }));
	process.env.PI_CODING_AGENT_DIR = dir;
	process.env.CPD_SESSION = "s";
	process.env.CPD_PLAN = "p";
	delete process.env.CPD_ATTEMPT;
	delete (globalThis as Record<symbol, unknown>)[FLAG];
	const written: string[] = [];
	const stderr = mock.method(process.stderr, "write", (chunk: unknown) => { written.push(String(chunk)); return true; });
	try {
		const mod = await import(INDEX.href);
		const load = async () => {
			const calls: string[] = [];
			const handlers: Handler[] = [];
			const pi = new Proxy({} as Record<string, unknown>, {
				get: (_t, method: string) => (...args: unknown[]) => {
					calls.push(method);
					if (method === "on" && args[0] === "session_start") handlers.push(args[1] as Handler);
				},
			});
			await mod.default(pi);
			return { calls, handlers };
		};
		const first = await load();
		assert.deepEqual(first.calls, ["on"]);
		const ctx = { mode: "print", ui: { notify: () => assert.fail("print mode does not notify") } };
		first.handlers.forEach((handler) => handler({ reason: "startup" }, ctx));
		const second = await load();
		second.handlers.forEach((handler) => handler({ reason: "reload" }, ctx));
		const file = join(dir, "settings.json");
		const line = `cpd ask_user_question: ${NAME}@2.13.8 is also loaded (${file}): cpd's own copy stands down and registers nothing. Remove it from packages in ${file} and run /reload.`;
		assert.deepEqual(written.join("").split("\n").filter((l) => l.includes("cpd ask_user_question:")), [line]);
	} finally {
		stderr.mock.restore();
		for (const [key, value] of saved) { if (value === undefined) delete process.env[key]; else process.env[key] = value; }
		delete (globalThis as Record<symbol, unknown>)[FLAG];
		rmSync(dir, { recursive: true, force: true });
	}
});

test("findOtherCopy finds the other copy while pi holds the lock on the settings files", async (t) => {
	const { findOtherCopy } = await import(DOUBLE.href);
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
	const agent = made("ask-double-lock-agent-");
	held(join(agent, "settings.json"), [`${NAME}@2.13.8`]);
	process.env.PI_CODING_AGENT_DIR = agent;
	assert.deepEqual(findOtherCopy(made("ask-double-lock-cwd-"))?.copy, { source: `${NAME}@2.13.8`, scope: "global" });
	const bare = made("ask-double-lock-bare-");
	writeFileSync(join(bare, "settings.json"), JSON.stringify({ packages: [] }));
	process.env.PI_CODING_AGENT_DIR = bare;
	const cwd = made("ask-double-lock-project-");
	mkdirSync(join(cwd, ".pi"));
	held(join(cwd, ".pi", "settings.json"), [`${NAME}@2.13.8`]);
	assert.deepEqual(findOtherCopy(cwd)?.copy, { source: `${NAME}@2.13.8`, scope: "project" });
});
