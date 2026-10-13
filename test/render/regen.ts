// Deliberate golden update: `CPD_REGEN_GOLDENS=1 node test/render/regen.ts`, then review `git diff test/render/golden`.
// `node --test` also executes this file (it lives under test/), so without CPD_REGEN_GOLDENS=1 it does nothing:
// it builds no fixture and writes no file. The golden tests only compare.
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

if (process.env.CPD_REGEN_GOLDENS !== "1") {
  console.log("test/render/regen.ts: no-op. To rewrite the goldens deliberately: CPD_REGEN_GOLDENS=1 node test/render/regen.ts");
} else {
  const { buildFixture, renderAll } = await import("./fixture.ts");
  const dir = fileURLToPath(new URL("./golden/", import.meta.url));
  await buildFixture();
  mkdirSync(dir, { recursive: true });
  for (const [name, text] of Object.entries(await renderAll())) writeFileSync(join(dir, name), text);
  console.log(`goldens written to ${dir}`);
}
