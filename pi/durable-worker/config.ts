export type Config = { home: string; session: string; plan: string; attempt: string; cwd: string; spec: string; model: string };

export function parseConfig(_argv: readonly string[], _env: Record<string, string | undefined>, _cwd: string): Config {
  throw new Error("not implemented");
}

export function storePath(_home: string, _plan: string, _attempt: string): string {
  throw new Error("not implemented");
}

export function reportPath(_home: string, _plan: string, _attempt: string): string {
  throw new Error("not implemented");
}
