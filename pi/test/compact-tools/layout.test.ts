/**
 * Where the compact-tools extension lives and what travels with it (docs/contract.md section 6.9, docs/implementing.md
 * "The compact-tools extension" points 1, 4 and 6): the manifest of the adapter lists it, the copy keeps upstream's file
 * names so a later upstream diff applies path for path, LICENSE and NOTICE sit beside it, the schema's defaults are the code's,
 * the root package.json runs these tests, and no `@ts-nocheck` is left on them once the sources are copied.
 */
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { DEFAULT_CONFIG } from "../../compact-tools/extensions/compact-tools-config.ts";

const PI = join(import.meta.dirname, "..", "..");
const ROOT = join(PI, "..");
const EXTENSION = join(PI, "compact-tools");
const json = (path: string) => JSON.parse(readFileSync(path, "utf8")) as Record<string, any>;

/** The fork's `extensions/*.ts`, but `compact-tools-release.ts` (its subject is gone), and the modules cpd adds: the cursor-blink, double-load and compaction-fold modules. */
const SOURCES = [
	"compact-tools-activity.ts", "compact-tools-balloon.ts", "compact-tools-claude.ts", "compact-tools-clock.ts", "compact-tools-codex-rows.ts",
	"compact-tools-codex.ts", "compact-tools-command.ts", "compact-tools-compaction.ts", "compact-tools-config.ts",
	"compact-tools-core.ts", "compact-tools-cursor.ts", "compact-tools-custom.ts", "compact-tools-double-load.ts", "compact-tools-grouping.ts",
	"compact-tools-hook.ts", "compact-tools-indices.ts", "compact-tools-invocation.ts", "compact-tools-italic.ts", "compact-tools-layout.ts", "compact-tools-loader.ts",
	"compact-tools-nested.ts", "compact-tools-progress.ts",
	"compact-tools-rendering.ts", "compact-tools-runtime.ts", "compact-tools-shell.ts", "compact-tools-silent.ts", "compact-tools-editor.ts",
	"compact-tools-thinking.ts", "compact-tools-transcript.ts", "compact-tools.ts", "compact-tools-types.ts",
	"compact-tools-viewport.ts",
];

test("pi/package.json lists the adapter and the compact-tools extension, each a file, and the skills as before", () => {
	const manifest = json(join(PI, "package.json"));
	assert.deepEqual(manifest.pi.extensions, ["./extension.ts", "./mcp-extension.ts", "./compact-tools/extensions/compact-tools.ts", "./ask-user-question/index.ts"]);
	assert.deepEqual(manifest.pi.skills, ["./skills"]);
	assert.deepEqual(manifest.pi.themes, ["./themes"]);
	// An entry whose file is missing makes pi fail to load the adapter: the entry and the files come in one change.
	for (const entry of manifest.pi.extensions as string[]) assert.ok(existsSync(join(PI, entry)), entry);
	assert.equal("dependencies" in manifest, false, "the package gains no dependency");
	assert.deepEqual(manifest.peerDependencies, { "@earendil-works/pi-coding-agent": "*", "@earendil-works/pi-tui": "*" });
});

test("extensions/ holds the fork's source files under their names, but compact-tools-release.ts, and the double-load module, importing nothing else", () => {
	const names = readdirSync(join(EXTENSION, "extensions")).filter((name) => name.endsWith(".ts")).sort();
	assert.deepEqual(names, [...SOURCES].sort());
	// ...which import only pi, pi-tui, node: modules, each other and the shared dependency-free atoms (lib/pill.ts, lib/time.ts, lib/italic.ts, lib/settings-read.ts), so the package gains no dependency.
	const found: string[] = [];
	for (const name of names) {
		const source = readFileSync(join(EXTENSION, "extensions", name), "utf8");
		for (const match of source.matchAll(/(?:from\s+|import\(\s*)["']([^"']+)["']/gu)) {
			const specifier = match[1]!;
			if (!(specifier.startsWith("./") || specifier === "../../lib/pill.ts" || specifier === "../../lib/time.ts" || specifier === "../../lib/italic.ts" || specifier === "../../lib/settings-read.ts" || specifier.startsWith("node:") || specifier === "@earendil-works/pi-coding-agent" || specifier === "@earendil-works/pi-tui")) found.push(`${name}: ${specifier}`);
		}
	}
	assert.deepEqual(found, []);
});

test("LICENSE is upstream's MIT text and NOTICE names the source, the base commit and the fork's", () => {
	const license = readFileSync(join(EXTENSION, "LICENSE"), "utf8");
	assert.match(license, /^MIT License\n\nCopyright \(c\) 2026 Dong Hyeong Lee\n/u);
	assert.match(license, /Permission is hereby granted, free of charge/u);
	const notice = readFileSync(join(EXTENSION, "NOTICE"), "utf8");
	assert.match(notice, /nedleeds\/pi-compact-tools/u);
	assert.match(notice, /8be1a8d/u, "the base commit, so an upstream diff can be taken");
	assert.match(notice, /0\.16\.6/u);
	assert.match(notice, /clayton-duarte\/pi-compact-tools/u);
	assert.match(notice, /2c1fcad8089f174738334ff99f2eb83fa43aedcf/u);
});

test("the schema's defaults are DEFAULT_CONFIG's", () => {
	const schema = json(join(EXTENSION, "schemas", "compact-tools.schema.json"));
	const properties = schema.properties as Record<string, { default?: unknown }>;
	for (const key of ["style", "mode", "fold", "toggle", "tools", "custom_tools", "previewLines"] as const) {
		assert.deepEqual(properties[key]!.default, DEFAULT_CONFIG[key], key);
	}
	const compact = properties.auto_compact as { properties: Record<string, { default?: unknown }> };
	for (const [tool, value] of Object.entries(DEFAULT_CONFIG.auto_compact)) assert.equal(compact.properties[tool]!.default, value, `auto_compact.${tool}`);
	const toggle = properties.toggle as unknown as { properties: Record<string, { default?: unknown }> };
	assert.equal(toggle.properties.expand!.default, DEFAULT_CONFIG.toggle.expand);
	assert.equal(toggle.properties.collapse!.default, DEFAULT_CONFIG.toggle.collapse);
});

test("the root package.json runs these tests with npm test, and pins the fake-timers packages they import", () => {
	const root = json(join(ROOT, "package.json"));
	assert.ok((root.scripts.test as string).includes("pi/test/compact-tools/*.test.ts"), "the default file list of `npm test`");
	assert.equal(root.devDependencies["@sinonjs/fake-timers"], "15.0.0");
	assert.equal(root.devDependencies["@types/sinonjs__fake-timers"], "15.0.1");
	assert.equal("tsx" in root.devDependencies, false, "they run with node --test, as every pi/test file does");
});

test("no test of the extension is left unchecked by TypeScript", () => {
	const directory = import.meta.dirname;
	const left = readdirSync(directory).filter((name) => name.endsWith(".ts") && /^\s*\/\/\s*@ts-nocheck/mu.test(readFileSync(join(directory, name), "utf8")));
	assert.deepEqual(left, [], "remove the @ts-nocheck line of each: the sources they import exist now");
});
