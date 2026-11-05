export interface RestoreExec {
  (
    file: string,
    args: string[],
    opts: { cwd: string; env: Record<string, string> },
  ): Promise<{ stdout: string }>;
}

export interface RestoreFs {
  cp(src: string, dst: string): Promise<void>;
  rm(path: string): Promise<void>;
  rename(a: string, b: string): Promise<void>;
  exists(p: string): Promise<boolean>;
}

export interface RestorePointsDeps {
  worktree: string;
  exec: RestoreExec;
  fs: RestoreFs;
  ref?: "refs/cpd/preview-last-good";
}

export interface RestorePoints {
  captureCandidate(): Promise<void>;
  promoteCandidate(): Promise<void>;
  hasLastGood(): Promise<boolean>;
  restoreLastGood(): Promise<void>;
  discardCandidate(): Promise<void>;
}

export function createRestorePoints(_deps: RestorePointsDeps): RestorePoints {
  throw new Error("not implemented");
}
