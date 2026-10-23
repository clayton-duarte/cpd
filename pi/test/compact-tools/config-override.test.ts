/**
 * compact-tools.json still overrides the defaults, as docs/contract.md section 6.9 specifies: `<agent dir>/compact-tools.json`,
 * then, for a trusted project only, `<cwd>/.pi/compact-tools.json`, each merged field by field over cpd's defaults
 * (fold "toggle", the cockpit's glyphs, seven tools), a bad file reported on stderr as `[compact-tools] <path>: <message>`.
 * The agent directory is pi's getAgentDir(), which reads PI_CODING_AGENT_DIR (set below).
 */
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { mock } from "node:test";
import { DEFAULT_CONFIG, loadConfig } from "../../compact-tools/extensions/compact-tools-config.ts";

// The agent directory of this file, pi's getAgentDir(). The harness points PI_CODING_AGENT_DIR at a directory of its own when it
// loads, so it is loaded when a test needs it (tests of the config alone do not), and the variable set back.
const AGENT_DIR = mkdtempSync(join(tmpdir(), "compact-tools-agent-"));
process.env.PI_CODING_AGENT_DIR = AGENT_DIR;
let harnessLoaded = false;
async function loadHarness() {
	const loaded = await import("./indicator-harness.ts");
	harnessLoaded = true;
	process.env.PI_CODING_AGENT_DIR = AGENT_DIR;
	return loaded;
}

const EXPAND = "\u{F0B26}";
const COLLAPSE = "\u{F0B2C}";
const SEVEN = ["read", "write", "edit", "bash", "grep", "find", "ls"];

const agentFile = () => join(process.env.PI_CODING_AGENT_DIR!, "compact-tools.json");
const plain = (line: string) => line.replace(/\x1b\[[0-9;]*m/gu, "");

/** A project directory, and the agent directory's file and the project's set to the given texts (none: no file). */
function files<T>(global: string | undefined, project: string | undefined, fn: (cwd: string) => T): T {
	const cwd = mkdtempSync(join(tmpdir(), "compact-tools-override-"));
	rmSync(agentFile(), { force: true });
	if (global !== undefined) writeFileSync(agentFile(), global);
	if (project !== undefined) {
		mkdirSync(join(cwd, ".pi"));
		writeFileSync(join(cwd, ".pi", "compact-tools.json"), project);
	}
	try { return fn(cwd); } finally { rmSync(cwd, { recursive: true, force: true }); rmSync(agentFile(), { force: true }); }
}

/** loadConfig's stderr lines. */
function stderrOf(fn: () => void): string[] {
	const errors = mock.method(console, "error", () => {});
	try { fn(); return errors.mock.calls.map((call) => call.arguments.join(" ")); } finally { errors.mock.restore(); }
}

test("a file that sets only style off keeps fold toggle, the glyphs and the seven tools", () => {
	files('{"style": "off"}', undefined, (cwd) => {
		const config = loadConfig(cwd, false);
		assert.equal(config.style, "off");
		assert.deepEqual({ ...config, style: "compact" }, DEFAULT_CONFIG);
		assert.equal(config.fold, "toggle");
		assert.deepEqual(config.toggle, { expand: EXPAND, collapse: COLLAPSE });
		assert.deepEqual(config.tools, SEVEN);
	});
});

test("fold: status brings back upstream's rows: a leading space, a └ closing row, no toggle", async () => {
	files('{"fold": "status"}', undefined, (cwd) => assert.equal(loadConfig(cwd, false).fold, "status"));
	const { advance, announce, CASES, loadExtension, makeRow, shutdown } = await loadHarness();
	const harness = await loadExtension({ fold: "status" }, { defaults: true });
	try {
		const id = "override-status";
		announce(harness, id, "bash");
		const row = makeRow("bash", id, CASES.bash!.args, harness.definitions.get("bash"));
		row.setArgsComplete();
		harness.handlers.get("tool_execution_start")!({ toolCallId: id, toolName: "bash", args: CASES.bash!.args });
		row.markExecutionStarted();
		advance(300);
		harness.handlers.get("tool_execution_end")!({ toolCallId: id, isError: false });
		row.updateResult({ ...CASES.bash!.success, isError: false } as never, false);
		const lines = row.render(100).map(plain).filter((line) => line.trim() !== "");
		assert.ok(lines[0]!.startsWith(" "), lines[0]);
		assert.match(lines.at(-1)!, /^ \u2514 Done in 0\.300s/u);
		assert.ok(lines.every((line) => !line.includes("\u2570") && !line.includes("\u256D") && !line.includes(EXPAND)), "no toggle");
	} finally { shutdown(harness); }
});

test("a file overrides field by field: tools, one glyph, previewLines", () => {
	files('{"tools": ["read"], "toggle": {"expand": "+"}, "previewLines": 5}', undefined, (cwd) => {
		const config = loadConfig(cwd, false);
		assert.deepEqual(config.tools, ["read"]);
		assert.deepEqual(config.toggle, { expand: "+", collapse: COLLAPSE }, "the other glyph is cpd's default");
		assert.equal(config.previewLines, 5);
		assert.equal(config.fold, "toggle");
		assert.equal(config.style, "compact");
	});
});

test("glyphs written as JSON escapes (surrogate pairs) are the same code points", () => {
	files('{"toggle": {"expand": "\\uDB80\\uDC4B", "collapse": "\\uDB80\\uDC63"}}', undefined, (cwd) => {
		assert.deepEqual(loadConfig(cwd, false).toggle, { expand: EXPAND, collapse: COLLAPSE });
	});
});

test("the project's file overrides the agent directory's, only when the project is trusted", () => {
	files('{"fold": "status", "tools": ["read", "bash"]}', '{"fold": "toggle", "tools": ["find"]}', (cwd) => {
		assert.deepEqual([loadConfig(cwd, false).fold, loadConfig(cwd, false).tools], ["status", ["read", "bash"]]);
		assert.deepEqual([loadConfig(cwd, true).fold, loadConfig(cwd, true).tools], ["toggle", ["find"]]);
	});
});

test("the project's file merges over the agent directory's, not over the defaults", () => {
	files('{"style": "codex", "fold": "status"}', '{"previewLines": 3}', (cwd) => {
		const config = loadConfig(cwd, true);
		assert.deepEqual([config.style, config.fold, config.previewLines], ["codex", "status", 3]);
	});
});

test("a file that does not parse is reported as [compact-tools] <path>: <message> and the defaults kept", () => {
	files("{not json", undefined, (cwd) => {
		let config = DEFAULT_CONFIG;
		const lines = stderrOf(() => { config = loadConfig(cwd, false); });
		assert.deepEqual(config, DEFAULT_CONFIG);
		assert.equal(lines.length, 1, lines.join("\n"));
		assert.ok(lines[0]!.startsWith(`[compact-tools] ${agentFile()}: `), lines[0]);
	});
});

test("an unknown fold is reported and keeps cpd's default, toggle, not upstream's status", () => {
	files('{"fold": "fancy"}', undefined, (cwd) => {
		let config = DEFAULT_CONFIG;
		const lines = stderrOf(() => { config = loadConfig(cwd, false); });
		assert.equal(config.fold, "toggle");
		assert.deepEqual(lines, [`[compact-tools] ${agentFile()}: fold must be one of status, toggle; using previous value`]);
	});
});

test("a bad project file is reported with its own path and the agent directory's values kept", () => {
	files('{"fold": "status"}', '{"style": "fancy"}', (cwd) => {
		let config = DEFAULT_CONFIG;
		const lines = stderrOf(() => { config = loadConfig(cwd, true); });
		assert.deepEqual([config.style, config.fold], ["compact", "status"]);
		assert.deepEqual(lines, [`[compact-tools] ${join(cwd, ".pi", "compact-tools.json")}: style must be one of compact, claude, codex, off; using previous value`]);
	});
});

test("an untrusted project's file is not read, so not reported either", () => {
	files(undefined, "{not json", (cwd) => {
		let config = DEFAULT_CONFIG;
		const lines = stderrOf(() => { config = loadConfig(cwd, false); });
		assert.deepEqual(config, DEFAULT_CONFIG);
		assert.deepEqual(lines, []);
	});
});

test.after(async () => {
	if (harnessLoaded) (await import("./indicator-harness.ts")).restoreClocks();
	rmSync(AGENT_DIR, { recursive: true, force: true });
});
