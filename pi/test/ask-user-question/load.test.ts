/** Loading the ask-user-question copy: the role gate and the adapter's package.json. */
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const INDEX = new URL("../../ask-user-question/index.ts", import.meta.url);
const PACKAGE = new URL("../../package.json", import.meta.url);

/** Registered tool names after running the factory with exactly `env` for the CPD_* variables. */
async function registeredToolsFor(env: Record<string, string>): Promise<{ name: string; exposure?: string }[]> {
	assert.ok(existsSync(INDEX), "pi/ask-user-question/index.ts exists");
	const tools: { name: string; exposure?: string }[] = [];
	const pi = new Proxy({} as Record<string, unknown>, {
		get: (_t, method: string) => (...args: unknown[]) => {
			if (method === "registerTool") tools.push(args[0] as { name: string; exposure?: string });
		},
	});
	const keys = ["CPD_SESSION", "CPD_PLAN", "CPD_ATTEMPT", "PI_CODING_AGENT_DIR"];
	const saved = keys.map((key) => [key, process.env[key]] as const);
	const agentDir = mkdtempSync(join(tmpdir(), "ask-load-"));
	for (const key of keys) delete process.env[key];
	Object.assign(process.env, env, { PI_CODING_AGENT_DIR: agentDir });
	try {
		const mod = await import(INDEX.href);
		await mod.default(pi);
	} finally {
		for (const [key, value] of saved) { if (value === undefined) delete process.env[key]; else process.env[key] = value; }
		rmSync(agentDir, { recursive: true, force: true });
	}
	return tools;
}

async function registeredFor(env: Record<string, string>): Promise<string[]> {
	return (await registeredToolsFor(env)).map((tool) => tool.name);
}

test("the tool registers for a Lead and a Manager only", async () => {
	assert.deepEqual(await registeredFor({ CPD_SESSION: "s", CPD_PLAN: "p" }), ["ask_user_question"], "Lead");
	assert.deepEqual(await registeredFor({ CPD_SESSION: "s" }), ["ask_user_question"], "Manager");
	assert.deepEqual(await registeredFor({ CPD_SESSION: "s", CPD_PLAN: "p", CPD_ATTEMPT: "a" }), [], "Worker");
	assert.deepEqual(await registeredFor({}), [], "none");
});

test("ask_user_question is registered model-only so codemode only keeps it declared", async () => {
	const [tool] = await registeredToolsFor({ CPD_SESSION: "s", CPD_PLAN: "p" });
	assert.equal(tool.name, "ask_user_question");
	assert.equal(tool.exposure, "model-only");
});

test("the adapter package lists the copy and every listed extension file exists", () => {
	const extensions: string[] = JSON.parse(readFileSync(PACKAGE, "utf8")).pi.extensions;
	assert.ok(extensions.includes("./ask-user-question/index.ts"), "lists ./ask-user-question/index.ts");
	for (const entry of extensions) assert.ok(existsSync(new URL(entry, PACKAGE)), `${entry} exists`);
	assert.ok(existsSync(INDEX), "pi/ask-user-question/index.ts exists");
});
