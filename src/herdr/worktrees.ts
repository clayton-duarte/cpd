import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { z } from "zod";
import { herdr } from "./herdr.ts";

const Opened = z.looseObject({ workspace: z.looseObject({ workspace_id: z.string() }) });
const WorkspaceList = z.looseObject({
  workspaces: z.array(
    z.looseObject({
      workspace_id: z.string(),
      worktree: z.looseObject({ checkout_path: z.string(), repo_root: z.string() }).optional(),
    }),
  ),
});

/** herdr reports realpaths (`/private/tmp/…` on macOS), so compare canonical paths. */
function canon(p: string): string {
  try {
    return realpathSync(p);
  } catch {
    return resolve(p);
  }
}

/** `herdr worktree open`; idempotent (t3 F3): an already-open worktree returns its existing workspace id. */
export async function openWorktree(o: { repo: string; path: string; label: string }): Promise<string> {
  const r = await herdr(["worktree", "open", "--cwd", o.repo, "--path", o.path, "--label", o.label, "--no-focus"], Opened);
  return r.workspace.workspace_id;
}

/** The workspace open on `path` (a worktree of `repo`), or null. */
export async function workspaceForWorktree(repo: string, path: string): Promise<string | null> {
  const { workspaces } = await herdr(["workspace", "list"], WorkspaceList);
  const want = canon(path);
  const wantRepo = canon(repo);
  const hit = workspaces.find((w) => w.worktree && canon(w.worktree.checkout_path) === want && canon(w.worktree.repo_root) === wantRepo);
  return hit ? hit.workspace_id : null;
}
