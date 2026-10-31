export type Config = { home: string; session: string; cwd: string; model: string };

export function parseConfig(_argv: readonly string[], _env: Record<string, string | undefined>, _cwd: string): Config {
  throw new Error("not implemented");
}

export function storePath(_home: string, _session: string): string {
  throw new Error("not implemented");
}
