import { describe, expect, it } from "vitest";
import { readStatus, type StatusDeps } from "./status.ts";

const SEP = "\u{f0798}";
interface Sources { version?: string; incident?: string; config?: string; workers?: string; preview?: string; install?: string }

function deps(s: Sources, env: Record<string, string> = {}, drift: StatusDeps["drift"] = async () => { throw new Error("drift must not be read"); }): StatusDeps {
  return {
    env,
    drift,
    async cpd(argv) {
      const a = argv.join(" ");
      if (a.includes("--version") && s.version !== undefined) return s.version;
      if (a.startsWith("incident") && s.incident !== undefined) return s.incident;
      if (a.startsWith("config") && s.config !== undefined) return s.config;
      throw new Error(`no source for cpd ${a}`);
    },
    async tmux(argv) {
      const a = argv.join(" ");
      if (a.includes("@cpd_workers") && s.workers !== undefined) return `${s.workers}\n`;
      if (a.includes("@cpd_preview") && s.preview !== undefined) return `${s.preview}\n`;
      throw new Error(`no source for tmux ${a}`);
    },
    async readFile(path) {
      return path.endsWith("run/cpd-install.json") ? (s.install ?? null) : null;
    },
  };
}

describe("status", () => {
  it("status reads the version from cpd --version", async () => {
    const st = await readStatus(deps({ version: '{"ok":true,"version":"0.21.45-070ddee","protocol":21}' }));
    expect(st.version).toBe("0.21.45-070ddee");
  });
  it("status reads the open incident start time", async () => {
    const on = '{"ok":true,"on":true,"incident":{"started_at":"2026-10-14T21:10:00.000Z","by":"x","reason":"r","plans":[],"until_workflow":null}}';
    expect((await readStatus(deps({ incident: on }))).incident).toEqual({ startedAt: "2026-10-14T21:10:00.000Z" });
    expect((await readStatus(deps({ incident: '{"ok":true,"on":false,"incident":null}' }))).incident).toBeNull();
  });
  it("status reads the runner from durable_workers", async () => {
    const cfg = (on: boolean) => `{"ok":true,"config":{"experimental":{"durable_workers":${on},"write_curation":true}}}`;
    expect((await readStatus(deps({ config: cfg(true) }))).runner).toBe("durable");
    expect((await readStatus(deps({ config: cfg(false) }))).runner).toBe("pi");
  });
  it("status reads workers queued waiting tenants and limit from the tmux option", async () => {
    const st = await readStatus(deps({ workers: `workers 52${SEP}13${SEP}10/10` }));
    expect(st.workers).toEqual({ queued: 52, waiting: 13, tenants: 10, limit: 10 });
  });
  it("status reads the preview sha from the tmux option", async () => {
    const st = await readStatus(deps({ preview: "my-worktree abc1234" }));
    expect(st.preview).toBe("my-worktree abc1234");
  });
  it("status reports a host alert from cpd-install.json", async () => {
    const skipped = '{"status":"skipped","reason":"dirty","since":"2026-01-01T00:00:00.000Z"}';
    const ok = '{"status":"ok","since":"2026-01-01T00:00:00.000Z"}';
    expect((await readStatus(deps({ install: skipped }))).hostAlert).toBe(true);
    expect((await readStatus(deps({ install: ok }))).hostAlert).toBe(false);
  });
  it("status reports nulls when a source is missing", async () => {
    const st = await readStatus(deps({}));
    expect(st).toEqual({ runner: null, preview: null, incident: null, workers: null, version: null, hostAlert: false, mode: "production", drift: null });
    const partial = await readStatus(deps({ version: '{"ok":true,"version":"1.2.3-dev"}' }));
    expect(partial).toEqual({ runner: null, preview: null, incident: null, workers: null, version: "1.2.3-dev", hostAlert: false, mode: "production", drift: null });
  });
  it("readStatus reports production mode and no drift by default", async () => {
    let reads = 0;
    const drift: StatusDeps["drift"] = async () => (reads += 1, { count: 1, oldestAt: null, stale: false });
    const st = await readStatus(deps({}, { CPD_CHECKOUT: "/w" }, drift));
    expect(st.mode).toBe("production");
    expect(st.drift).toBeNull();
    expect(reads).toBe(0);
    await expect(readStatus(deps({}, { CPD_MODE: "staging" }, drift))).rejects.toThrow('CPD_MODE must be production or preview, got "staging"');
  });
  it("readStatus reports preview mode with the drift summary of the checkout", async () => {
    const roots: string[] = [];
    const summary = { count: 2, oldestAt: "2026-11-05T10:00:00.000Z", stale: true };
    const drift: StatusDeps["drift"] = async (root) => (roots.push(root), { ...summary, entries: [{ kind: "dirty", ref: "a.ts", at: summary.oldestAt }] } as typeof summary);
    const st = await readStatus(deps({}, { CPD_MODE: "preview", CPD_CHECKOUT: "/w" }, drift));
    expect(roots).toEqual(["/w"]);
    expect(st.mode).toBe("preview");
    expect(st.drift).toEqual(summary);
  });
});
