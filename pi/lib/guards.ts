/**
 * Typed tool guards and tool hooks (docs/architecture/pi-native-core.md §4.6).
 * Inspects structured tool arguments directly instead of regex scanning bash text.
 */

import { homedir } from "node:os";
import { primaryCheckoutWrite } from "./mainguard.ts";

export interface ToolCallTarget {
  toolName: string;
  input: unknown;
  cwd?: string;
  home?: string;
  role?: string;
  guardedTestFiles?: string[];
}

export interface GuardVerdict {
  blocked: boolean;
  reason?: string;
  protocol?: string;
  source?: string;
}

/**
 * Validates whether a tool call violates test immutability or primary checkout rules.
 */
export function validateToolCall(call: ToolCallTarget): GuardVerdict {
  const { toolName, input, cwd = process.cwd(), home = homedir(), role, guardedTestFiles = [] } = call;

  // 1. mainguard: check primary checkout immutability on write/edit tools
  if (toolName === "edit" || toolName === "write") {
    const mainBreach = primaryCheckoutWrite(toolName, input, cwd, home);
    if (mainBreach !== undefined) {
      return {
        blocked: true,
        reason: mainBreach,
        protocol: "primary checkout",
        source: "mainguard",
      };
    }
  }

  // 2. testguard: red test file immutability enforced directly by tool hook for implementation Jobs
  if (role === "worker" && (toolName === "edit" || toolName === "write")) {
    const rawPath = (input as { path?: unknown } | null | undefined)?.path;
    if (typeof rawPath === "string" && rawPath !== "") {
      const targetPath = rawPath.startsWith("@") ? rawPath.slice(1) : rawPath;
      for (const guarded of guardedTestFiles) {
        if (targetPath === guarded || targetPath.endsWith("/" + guarded) || (guarded.endsWith("/") && targetPath.includes(guarded))) {
          return {
            blocked: true,
            reason: `φ: red test files are immutable for implementation jobs; refused: ${targetPath}`,
            protocol: "red tests",
            source: "testguard",
          };
        }
      }
    }
  }

  return { blocked: false };
}

/**
 * Indicates whether typed tool guards are active.
 */
export const toolGuardActive = true;
