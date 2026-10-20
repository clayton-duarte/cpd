/**
 * pi/lib/render.ts reads the style from the extension's effective config (docs/contract.md section 6.9 "One source of truth for
 * the style", docs/implementing.md "The compact-tools extension" point 3): `loadStyle(project)` is
 * `loadConfig(project.cwd, project.trusted).style`, the agent directory is pi's getAgentDir() (PI_CODING_AGENT_DIR), and
 * render.ts has no copy of the style parsing. The calls below are written as the contract's signatures and cast, because
 * render.ts's `env` parameters go with the change; pi/test/render.test.ts keeps the style rules of commands.md section 15.9.
 */
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { loadStyle, Rendering } from "../../lib/render.ts";

type Project = { cwd: string; trusted: boolean };
const loadStyleOf = loadStyle as unknown as (project: Project) => string;
const loadOf = (rendering: Rendering) => (rendering as unknown as { load(project: Project): void }).load.bind(rendering);

// pi's agent directory for this file: getAgentDir() reads it (as pi/test/fake.ts sets it).
process.env.PI_CODING_AGENT_DIR = mkdtempSync(join(tmpdir(), "compact-tools-render-agent-"));

const RENDER = readFileSync(join(import.meta.dirname, "..", "..", "lib", "render.ts"), "utf8");
const agentFile = () => join(process.env.PI_CODING_AGENT_DIR!, "compact-tools.json");

/** The agent directory's file and the project's set to the given texts (none: no file). */
function files<T>(global: string | undefined, project: string | undefined, fn: (project: (trusted: boolean) => Project) => T): T {
	const cwd = mkdtempSync(join(tmpdir(), "compact-tools-render-"));
	rmSync(agentFile(), { force: true });
	if (global !== undefined) writeFileSync(agentFile(), global);
	if (project !== undefined) {
		mkdirSync(join(cwd, ".pi"));
		writeFileSync(join(cwd, ".pi", "compact-tools.json"), project);
	}
	try { return fn((trusted) => ({ cwd, trusted })); } finally { rmSync(cwd, { recursive: true, force: true }); rmSync(agentFile(), { force: true }); }
}

test("render.ts takes the style from the extension's loadConfig and keeps no copy of its parsing", () => {
	assert.match(RENDER, /import \{[^}]*\bloadConfig\b[^}]*\} from "\.\.\/compact-tools\/extensions\/compact-tools-config\.ts"/u);
	assert.match(RENDER, /loadConfig\([^)]*\)\.style/u);
	for (const gone of ["STYLES", "DEFAULT_STYLE", "CONFIG_FILE", "styleFrom", "agentDirOf", "const warn"]) {
		assert.equal(RENDER.includes(gone), false, `${gone} is the extension's, not render.ts's`);
	}
	assert.match(RENDER, /DisplayStyle/u, "the type Style is the extension's DisplayStyle");
});

test("loadStyle(project) reads the agent directory pi's getAgentDir() names, PI_CODING_AGENT_DIR, with no env argument", () => {
	files('{"style": "codex"}', undefined, (project) => assert.equal(loadStyleOf(project(false)), "codex"));
	assert.equal(loadStyle.length, 1, "one parameter: the project");
});

test("loadStyle(project) is the extension's effective style: the default compact, a trusted project's file over the agent directory's", () => {
	files(undefined, undefined, (project) => assert.equal(loadStyleOf(project(true)), "compact"));
	files('{"style": "claude"}', '{"style": "codex"}', (project) => {
		assert.equal(loadStyleOf(project(false)), "claude");
		assert.equal(loadStyleOf(project(true)), "codex");
	});
	files('{"style": "off", "fold": "status"}', '{"previewLines": 3}', (project) => assert.equal(loadStyleOf(project(true)), "off", "a project file without style keeps the one before it"));
});

test("Rendering.load(project) reads the style once, from the same config", () => {
	files('{"style": "claude"}', undefined, (project) => {
		const rendering = new Rendering();
		assert.equal(rendering.style, "compact", "before session_start: the extension's default");
		loadOf(rendering)(project(false));
		assert.equal(rendering.style, "claude");
	});
});

test.after(() => rmSync(process.env.PI_CODING_AGENT_DIR!, { recursive: true, force: true }));
