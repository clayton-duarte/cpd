import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

const ROOT = join(import.meta.dirname, "..");
const DIRS = ["lib", "compact-tools/extensions", "ask-user-question", "themes"];
const BLACK = ["[30m", "[38;5;0m", "\\e[30m", "\\u001b[30m"];

const files = (dir: string): string[] =>
	readdirSync(dir).flatMap((name) => {
		if (name === "node_modules") return [];
		const path = join(dir, name);
		if (statSync(path).isDirectory()) return files(path);
		return /\.(ts|json)$/.test(name) ? [path] : [];
	});

test("no pi source or theme names colour 0 or black as a text colour", () => {
	const found: string[] = [];
	for (const dir of DIRS) {
		for (const path of files(join(ROOT, dir))) {
			readFileSync(path, "utf8")
				.split("\n")
				.forEach((line, i) => {
					if (BLACK.some((b) => line.includes(b))) found.push(`${path}:${i + 1}`);
				});
		}
	}
	for (const path of files(join(ROOT, "themes")).filter((p) => p.endsWith(".json"))) {
		const colors = JSON.parse(readFileSync(path, "utf8")).colors as Record<string, number | string>;
		for (const [token, value] of Object.entries(colors)) {
			if (!token.endsWith("Bg") && value === 0) found.push(`${path}:${token}`);
		}
	}
	assert.deepEqual(found, []);
});
