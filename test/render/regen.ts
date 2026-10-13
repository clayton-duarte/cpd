// Deliberate golden update: `node test/render/regen.ts`, then review `git diff test/render/golden`. Never run by the tests.
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { buildFixture, renderAll } from "./fixture.ts";

const dir = fileURLToPath(new URL("./golden/", import.meta.url));
await buildFixture();
mkdirSync(dir, { recursive: true });
for (const [name, text] of Object.entries(await renderAll())) writeFileSync(join(dir, name), text);
console.log(`goldens written to ${dir}`);
