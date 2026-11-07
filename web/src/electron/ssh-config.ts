// Ssh config reading, the host source of the remote TUI tabs (ELECTRON.md, `## Remote TUI tabs over ssh`).
//
// parseSshConfig(text) returns the ssh hosts of one config's text, in file order:
//   - one entry per non-wildcard pattern of a `Host` block that declares a `HostName`;
//   - a pattern that contains `*`, `?` or `!` is a wildcard and is skipped;
//   - a block without a `HostName` yields nothing;
//   - aliases keep first-seen order and a duplicate alias is dropped (the first wins).
//
// resolveSshHosts(configPath, deps) returns the same for a config file: it reads the file and
// splices each `Include` file in at the position of the directive, depth-first. An include path
// is resolved relative to the directory of the file that includes it, `~/` expands to `deps.home`,
// a path is read at most once and a missing include is skipped. `deps` defaults to a real reader
// over `node:fs` and `os.homedir()`.

export interface SshHost {
  alias: string;
  hostName: string;
}

export interface SshConfigDeps {
  /** Reads a file's text; rejects with an error whose `code` is `"ENOENT"` when it is missing. */
  readFile(path: string): Promise<string>;
  /** The directory `~/` expands to. */
  home: string;
}

export function parseSshConfig(_text: string): SshHost[] {
  throw new Error("not implemented");
}

export function resolveSshHosts(_configPath: string, _deps?: SshConfigDeps): Promise<SshHost[]> {
  throw new Error("not implemented");
}
