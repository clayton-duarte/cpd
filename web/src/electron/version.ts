// Shell version and minimum ABI declarations (docs/implementing.md "The shell release and the payload")

export const SHELL_VERSION = "0.1.0";
export const SHELL_ABI_VERSION = 2;
export const MIN_SHELL_ABI_VERSION = 1;

export interface AbiCheckResult {
  compatible: boolean;
  error?: string;
}

export function checkAbi(shellAbi: number, payloadMinAbi: number): AbiCheckResult {
  if (shellAbi < payloadMinAbi) {
    return {
      compatible: false,
      error: "new CPD.app release needed",
    };
  }
  return { compatible: true };
}

export interface VersionBumpCheckResult {
  ok: boolean;
  error?: string;
}

export function checkShellVersionBump(changedFiles: string[], versionBumped: boolean): VersionBumpCheckResult {
  const isShellFile = (file: string): boolean => {
    return (
      file.startsWith("web/src/electron/") ||
      file.startsWith("web/scripts/") ||
      file === "web/electron-builder.yml" ||
      file === "web/vite.electron.config.ts"
    );
  };

  const hasShellChanges = changedFiles.some(isShellFile);
  if (hasShellChanges && !versionBumped) {
    return {
      ok: false,
      error: "shell code changed without version bump (web/src/electron/version.ts)",
    };
  }

  return { ok: true };
}
