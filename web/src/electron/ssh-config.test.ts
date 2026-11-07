import { parseSshConfig, resolveSshHosts } from "./ssh-config.ts";
import type { SshConfigDeps } from "./ssh-config.ts";

function fakeFs(files: Record<string, string>): { deps: SshConfigDeps; reads: string[] } {
  const reads: string[] = [];
  const deps: SshConfigDeps = {
    home: "/home/u",
    readFile: (path) => {
      reads.push(path);
      const text = files[path];
      if (text === undefined) {
        return Promise.reject(Object.assign(new Error(`ENOENT: ${path}`), { code: "ENOENT" }));
      }
      return Promise.resolve(text);
    },
  };
  return { deps, reads };
}

const lines = (...rows: string[]) => rows.join("\n");

test("parses a Host block with a HostName", () => {
  const text = lines(
    "# the home server",
    "Host homeserver",
    "  HostName 10.0.0.5",
    "  User clay",
    "",
    "Host alpha beta",
    "  HostName shared.example",
  );
  expect(parseSshConfig(text)).toEqual([
    { alias: "homeserver", hostName: "10.0.0.5" },
    { alias: "alpha", hostName: "shared.example" },
    { alias: "beta", hostName: "shared.example" },
  ]);
});

test("skips a Host block without a HostName", () => {
  const text = lines("Host nohost", "  User clay", "", "Host named", "  HostName n.example");
  expect(parseSshConfig(text)).toEqual([{ alias: "named", hostName: "n.example" }]);
});

test("skips wildcard Host patterns", () => {
  const text = lines(
    "Host *",
    "  HostName star.example",
    "Host ?x",
    "  HostName q.example",
    "Host !excluded",
    "  HostName excluded.example",
    "Host web *",
    "  HostName w.example",
    "Host real",
    "  HostName r.example",
  );
  expect(parseSshConfig(text)).toEqual([
    { alias: "web", hostName: "w.example" },
    { alias: "real", hostName: "r.example" },
  ]);
});

test("resolves Include files transitively in place", async () => {
  const f = fakeFs({
    "/cfg/config": lines(
      "Host alpha",
      "  HostName a.example",
      "Include inc/mid",
      "Host omega",
      "  HostName o.example",
    ),
    "/cfg/inc/mid": lines(
      "Host beta",
      "  HostName b.example",
      "Include deep",
      "Host gamma",
      "  HostName g.example",
    ),
    "/cfg/inc/deep": lines("Host delta", "  HostName d.example"),
  });
  await expect(resolveSshHosts("/cfg/config", f.deps)).resolves.toEqual([
    { alias: "alpha", hostName: "a.example" },
    { alias: "beta", hostName: "b.example" },
    { alias: "delta", hostName: "d.example" },
    { alias: "gamma", hostName: "g.example" },
    { alias: "omega", hostName: "o.example" },
  ]);
  expect(f.reads).toEqual(["/cfg/config", "/cfg/inc/mid", "/cfg/inc/deep"]);
});

test("keeps first-seen alias order and drops duplicates", () => {
  const text = lines(
    "Host beta",
    "  HostName b.example",
    "Host alpha",
    "  HostName a.example",
    "Host beta",
    "  HostName b2.example",
    "Host *",
    "  HostName star.example",
  );
  expect(parseSshConfig(text)).toEqual([
    { alias: "beta", hostName: "b.example" },
    { alias: "alpha", hostName: "a.example" },
  ]);
});
