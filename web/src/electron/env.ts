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
/** Stub: the caller's cpd session identity is not yet stripped (red test). */
export function stripSessionEnv(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  return { ...env };
}
export async function resolveLoginPath(options: LoginPathOptions): Promise<string> {
  if (options.platform !== "darwin") {
    return options.currentPath ?? "";
  }
  const script = `printf '%s' '${PATH_BEGIN}'; printf '%s' "$PATH"; printf '%s' '${PATH_END}'`;
  const stdout = await options.run(options.shell ?? "/bin/zsh", ["-ilc", script]);
  return parseMarkedPath(stdout);
}
