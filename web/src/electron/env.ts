export const PATH_BEGIN = "__CPD_PATH_BEGIN__";
export const PATH_END = "__CPD_PATH_END__";
export type ShellRunner = (file: string, args: readonly string[]) => Promise<string>;
export interface LoginPathOptions {
  platform: NodeJS.Platform;
  shell: string | undefined;
  currentPath: string | undefined;
  run: ShellRunner;
}
export function parseMarkedPath(stdout: string): string {
  const begin = stdout.indexOf(PATH_BEGIN);
  const end = begin === -1 ? -1 : stdout.indexOf(PATH_END, begin + PATH_BEGIN.length);
  if (begin === -1 || end === -1) {
    throw new Error("login shell printed no PATH marker");
  }
  return stdout.slice(begin + PATH_BEGIN.length, end);
}
/**
 * The one canonical list of the per-session cpd variables (contract §6.6),
 * mirroring core's `SESSION_ENV_KEYS` (core/src/host/pi/launch.rs).
 */
export const SESSION_ENV_KEYS = [
  "CPD_SESSION",
  "CPD_PLAN",
  "CPD_ATTEMPT",
  "CPD_QUIT_MARKER",
  "CPD_JOB_KIND",
] as const;

/**
 * A copy of `env` with exactly the per-session cpd variables removed; every
 * other variable — `CPD_HOME`, `PATH` — stays (contract §6.6).
 */
export function stripSessionEnv(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const stripped = { ...env };
  for (const key of SESSION_ENV_KEYS) delete stripped[key];
  return stripped;
}
export async function resolveLoginPath(options: LoginPathOptions): Promise<string> {
  if (options.platform !== "darwin") {
    return options.currentPath ?? "";
  }
  const script = `printf '%s' '${PATH_BEGIN}'; printf '%s' "$PATH"; printf '%s' '${PATH_END}'`;
  const stdout = await options.run(options.shell ?? "/bin/zsh", ["-ilc", script]);
  return parseMarkedPath(stdout);
}
