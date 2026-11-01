// The Rust wall-clock scan of no-fixed-sleeps.test.ts. The patterns are built from parts so that this file holds none of the scanned text.
export function rustWallClockPatterns(): RegExp[] {
  return [new RegExp("thread::" + "sleep\\("), new RegExp("Instant::" + "now\\("), new RegExp("SystemTime::" + "now\\(")];
}

// Whether a file of `<crate>/src` is read whole (a test file) rather than from its first `#[cfg(test)]`.
export function scansWhole(crate: string, relPathInSrc: string): boolean {
  void crate;
  const parts = relPathInSrc.split("/");
  return parts[parts.length - 1] === "tests.rs" || parts.slice(0, -1).includes("tests");
}
