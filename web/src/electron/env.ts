export const PATH_BEGIN = "__CPD_PATH_BEGIN__";
export const PATH_END = "__CPD_PATH_END__";
export type ShellRunner = (file: string, args: readonly string[]) => Promise<string>;
export interface LoginPathOptions {
  platform: NodeJS.Platform;
  shell: string | undefined;
  currentPath: string | undefined;
  run: ShellRunner;
}
export function parseMarkedPath(_stdout: string): string {
  throw new Error("not implemented");
}
export function resolveLoginPath(_options: LoginPathOptions): Promise<string> {
  throw new Error("not implemented");
}
