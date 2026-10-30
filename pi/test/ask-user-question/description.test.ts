/** The texts built from LIMIT (docs/contract.md, docs/implementing.md: the ask-user-question copy). Every sentence is built here from LIMIT. */
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { LIMIT } from "../../lib/askguard.ts";

const ROOT = new URL("../../ask-user-question/", import.meta.url);
const INDEX = new URL("index.ts", ROOT);
const SENTENCE = `The whole call holds at most ${LIMIT} characters, summed over every question, header, option label, description and preview; a longer call is refused before the User sees it. Put larger content in a file outside the repo and point to it in chat.`;
const NOTE = `Counts toward the call's ${LIMIT}-character total.`;

type Tool = { name: string; description: string; promptGuidelines: string[]; parameters: any };

/** Registers the factory on a fake pi as a Lead and returns the tool it registers. */
async function registeredTool(): Promise<Tool> {
	assert.ok(existsSync(INDEX), "pi/ask-user-question/index.ts exists");
	const tools: Tool[] = [];
	const pi = new Proxy({} as Record<string, unknown>, {
		get: (_t, method: string) => (...args: unknown[]) => { if (method === "registerTool") tools.push(args[0] as Tool); },
	});
	const saved = { s: process.env.CPD_SESSION, a: process.env.CPD_ATTEMPT, d: process.env.PI_CODING_AGENT_DIR };
	const agentDir = mkdtempSync(join(tmpdir(), "ask-desc-"));
	process.env.PI_CODING_AGENT_DIR = agentDir;
	process.env.CPD_SESSION = "lead-session";
	delete process.env.CPD_ATTEMPT;
	try {
		const mod = await import(INDEX.href);
		await mod.default(pi);
	} finally {
		if (saved.s === undefined) delete process.env.CPD_SESSION; else process.env.CPD_SESSION = saved.s;
		if (saved.a === undefined) delete process.env.CPD_ATTEMPT; else process.env.CPD_ATTEMPT = saved.a;
		if (saved.d === undefined) delete process.env.PI_CODING_AGENT_DIR; else process.env.PI_CODING_AGENT_DIR = saved.d;
		rmSync(agentDir, { recursive: true, force: true });
	}
	const tool = tools.find((candidate) => candidate.name === "ask_user_question");
	assert.ok(tool, "ask_user_question is registered");
	return tool;
}

test("the tool description and the prompt guideline end with the sentence built from LIMIT", async () => {
	const tool = await registeredTool();
	assert.ok(tool.description.endsWith(`\n\n${SENTENCE}`), "description ends with a blank line and the sentence");
	assert.equal(tool.promptGuidelines.at(-1), SENTENCE);
});

test("the question and preview field descriptions end with the note built from LIMIT", async () => {
	const tool = await registeredTool();
	const item = tool.parameters.properties.questions.items.properties;
	assert.ok(item.question.description.endsWith(` ${NOTE}`), "question");
	assert.ok(item.options.items.properties.preview.description.endsWith(` ${NOTE}`), "preview");
});

/** Source with `//` and block comments blanked, string contents kept. */
function withoutComments(source: string): string {
	return source.replace(/("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|`(?:\\.|[^`\\])*`)|\/\*[\s\S]*?\*\/|\/\/[^\n]*/gu, (match, str) => (str ? match : ""));
}

function tsFiles(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
		entry.isDirectory() ? tsFiles(join(dir, entry.name)) : entry.name.endsWith(".ts") ? [join(dir, entry.name)] : []);
}

test("no literal 1500 in pi/ask-user-question outside comments", () => {
	assert.ok(existsSync(INDEX), "pi/ask-user-question/index.ts exists");
	const files = tsFiles(ROOT.pathname);
	assert.ok(files.length > 20, `more than 20 .ts files, found ${files.length}`);
	for (const file of files) assert.ok(!/1500/u.test(withoutComments(readFileSync(file, "utf8"))), `${file} writes 1500`);
});
