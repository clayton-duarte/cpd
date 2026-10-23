/**
 * The copy of @juicesharp/rpiv-ask-user-question under pi/ask-user-question: LICENSE and NOTICE beside it, policy
 * neutrality, and imports limited to pi, pi-tui, typebox, node and itself.
 */
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const DIR = fileURLToPath(new URL("../../ask-user-question/", import.meta.url));
const BASE_COMMIT = "afc21bc25f7d07cae9fb3103dc309240a16b9f49";

function tsFiles(dir: string): string[] {
	const out: string[] = [];
	for (const e of readdirSync(dir, { withFileTypes: true })) {
		const p = join(dir, e.name);
		if (e.isDirectory()) out.push(...tsFiles(p));
		else if (e.name.endsWith(".ts")) out.push(p);
	}
	return out.sort();
}
const copyFiles = (): string[] => {
	assert.ok(existsSync(DIR), "pi/ask-user-question exists");
	const files = tsFiles(DIR);
	assert.ok(files.length > 20, "the guard must find the files it guards");
	return files;
};

test("LICENSE and NOTICE exist and NOTICE names the base commit", () => {
	assert.ok(existsSync(join(DIR, "LICENSE")), "pi/ask-user-question/LICENSE exists");
	assert.ok(existsSync(join(DIR, "NOTICE")), "pi/ask-user-question/NOTICE exists");
	assert.ok(readFileSync(join(DIR, "NOTICE"), "utf8").includes(BASE_COMMIT), "NOTICE names the base commit");
});

test("policy-neutral: no record id, owner or machine fact in pi/ask-user-question", () => {
	const files = copyFiles();
	const forbidden = [/\bhs-[dnqap][0-9]+/, /clayton-duarte/, /missionlane/, /homeserver/, /User's machines/];
	const hits: string[] = [];
	for (const f of files) {
		readFileSync(f, "utf8")
			.split("\n")
			.forEach((line, i) => {
				for (const re of forbidden) if (re.test(line)) hits.push(`${f}:${i + 1}: ${re} in: ${line.trim().slice(0, 100)}`);
			});
	}
	assert.deepEqual(hits, []);
});

test("the copy imports nothing outside pi, pi-tui, typebox, node and itself", () => {
	const files = copyFiles();
	const allowed = (s: string): boolean =>
		s.startsWith("node:") ||
		s.startsWith("./") ||
		s.startsWith("../") ||
		s === "@earendil-works/pi-coding-agent" ||
		s === "@earendil-works/pi-tui" ||
		s === "typebox" ||
		s.startsWith("typebox/");
	const patterns = [/\bfrom\s+["']([^"']+)["']/g, /\bimport\s+["']([^"']+)["']/g, /\bimport\(\s*["']([^"']+)["']\s*\)/g];
	const bad: string[] = [];
	for (const f of files) {
		const text = readFileSync(f, "utf8");
		for (const re of patterns) for (const m of text.matchAll(re)) if (!allowed(m[1]!)) bad.push(`${f}: ${m[1]}`);
	}
	assert.deepEqual(bad, []);
});
