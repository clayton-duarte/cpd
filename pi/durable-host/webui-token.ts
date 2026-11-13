import { timingSafeEqual } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

function processAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

/** Contract §6.16: read live servers' token files on every check, never the shared legacy file. */
export async function webuiVerified(home: string, presented: string | undefined, alive = processAlive): Promise<boolean> {
  if (presented === undefined || presented === "") return false;
  const dir = join(home, "run", "webui-tokens");
  let files: string[];
  try { files = await readdir(dir); } catch { return false; }
  const given = Buffer.from(presented);
  for (const file of files) {
    if (!/^[1-9]\d*\.token$/.test(file)) continue;
    const pid = Number(file.slice(0, -".token".length));
    if (!Number.isSafeInteger(pid) || pid > 2_147_483_647 || !alive(pid)) continue;
    try {
      const actual = await readFile(join(dir, file));
      if (given.length === actual.length && timingSafeEqual(given, actual)) return true;
    } catch { /* A removed or unreadable file is not a verified server token. */ }
  }
  return false;
}
