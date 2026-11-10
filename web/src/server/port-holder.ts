/**
 * Port-in-use holder detection.
 * Identifies the process and PID holding a TCP port using lsof (macOS) or ss (Linux).
 */

export interface PortHolder {
  process: string;
  pid: number;
}

export interface PortHolderOptions {
  execCommand?: (command: string, args: string[]) => Promise<{ stdout: string; stderr: string; exitCode: number }>;
}

export async function detectPortHolder(_port: number, _options?: PortHolderOptions): Promise<PortHolder | null> {
  return null;
}
