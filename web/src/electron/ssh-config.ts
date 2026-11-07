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

import * as os from "node:os";
import * as path from "node:path";
import { readFile } from "node:fs/promises";

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

interface Block {
  patterns: string[];
  hostName: string | undefined;
}

// A `#` starts a comment only at the start of the line or after whitespace, so `a#b` is a value.
function stripComment(line: string): string {
  for (let i = 0; i < line.length; i += 1) {
    if (line[i] === "#" && (i === 0 || /\s/.test(line[i - 1] ?? ""))) return line.slice(0, i);
  }
  return line;
}

function splitKeyValue(line: string): { key: string; value: string } | null {
  const match = /^(\S+)(?:\s+(.*))?$/.exec(line);
  if (!match) return null;
  return { key: match[1] ?? "", value: (match[2] ?? "").trim() };
}

function isKey(word: string, key: string): boolean {
  return word.toLowerCase() === key;
}

function isWildcard(pattern: string): boolean {
  return pattern.includes("*") || pattern.includes("?") || pattern.includes("!");
}

function patternsOf(value: string): string[] {
  return value.split(/\s+/).filter((token) => token !== "");
}

function isMissing(error: unknown): boolean {
  return typeof error === "object" && error !== null && (error as { code?: unknown }).code === "ENOENT";
}

export function parseSshConfig(text: string): SshHost[] {
  const hosts: SshHost[] = [];
  const seen = new Set<string>();
  let block: Block | null = null;
  const flush = (): void => {
    if (block && block.hostName !== undefined) {
      const hostName = block.hostName;
      for (const pattern of block.patterns) {
        if (isWildcard(pattern) || seen.has(pattern)) continue;
        seen.add(pattern);
        hosts.push({ alias: pattern, hostName });
      }
    }
    block = null;
  };
  for (const raw of text.split(/\r?\n/)) {
    const line = stripComment(raw).trim();
    if (line === "") continue;
    const entry = splitKeyValue(line);
    if (!entry) continue;
    if (isKey(entry.key, "host")) {
      flush();
      block = { patterns: patternsOf(entry.value), hostName: undefined };
    } else if (block && isKey(entry.key, "hostname") && block.hostName === undefined) {
      block.hostName = entry.value;
    }
  }
  flush();
  return hosts;
}

function resolveInclude(token: string, dir: string, home: string): string {
  if (token === "~") return home;
  if (token.startsWith("~/")) return path.join(home, token.slice(2));
  if (path.isAbsolute(token)) return token;
  return path.resolve(dir, token);
}

export async function resolveSshHosts(configPath: string, deps?: SshConfigDeps): Promise<SshHost[]> {
  const reader: SshConfigDeps =
    deps ?? { readFile: (file) => readFile(file, "utf8"), home: os.homedir() };
  const readPaths = new Set<string>();
  const seenAliases = new Set<string>();
  const hosts: SshHost[] = [];

  const pushBlock = (block: Block): void => {
    if (block.hostName === undefined) return;
    const hostName = block.hostName;
    for (const pattern of block.patterns) {
      if (isWildcard(pattern) || seenAliases.has(pattern)) continue;
      seenAliases.add(pattern);
      hosts.push({ alias: pattern, hostName });
    }
  };

  const visit = async (filePath: string): Promise<void> => {
    if (readPaths.has(filePath)) return;
    readPaths.add(filePath);
    let text: string;
    try {
      text = await reader.readFile(filePath);
    } catch (error) {
      if (isMissing(error)) return;
      throw error;
    }
    const dir = path.dirname(filePath);
    let block: Block | null = null;
    const flush = (): void => {
      if (block) pushBlock(block);
      block = null;
    };
    for (const raw of text.split(/\r?\n/)) {
      const line = stripComment(raw).trim();
      if (line === "") continue;
      const entry = splitKeyValue(line);
      if (!entry) continue;
      if (isKey(entry.key, "host")) {
        flush();
        block = { patterns: patternsOf(entry.value), hostName: undefined };
      } else if (isKey(entry.key, "include")) {
        flush();
        for (const token of patternsOf(entry.value)) {
          await visit(resolveInclude(token, dir, reader.home));
        }
      } else if (block && isKey(entry.key, "hostname") && block.hostName === undefined) {
        block.hostName = entry.value;
      }
    }
    flush();
  };

  await visit(configPath);
  return hosts;
}
