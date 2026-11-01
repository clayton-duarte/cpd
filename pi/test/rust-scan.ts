// The Rust wall-clock scan of no-fixed-sleeps.test.ts. The patterns are built from parts so that this file holds none of the scanned text.
export function rustWallClockPatterns(): RegExp[] {
  return [
    new RegExp("thread::" + "sleep\\("),
    new RegExp("Instant::" + "now\\("),
    new RegExp("SystemTime::" + "now\\("),
    // a free function `sleep(` (after a `use`): not a `.sleep(` method call (a fake clock's) and not a path call (`Clock::sleep(`), a string or `fn sleep(`
    new RegExp("(?<![\\w.:\"])(?<!fn )" + "sleep\\("),
    new RegExp("tokio::time::" + "sleep\\("),
  ];
}

// Whether a file of `<crate>/src` is read whole (a test file) rather than from its first `#[cfg(test)]`.
export function scansWhole(crate: string, relPathInSrc: string): boolean {
  void crate;
  const parts = relPathInSrc.split("/");
  const name = parts[parts.length - 1];
  return name === "tests.rs" || name.endsWith("_tests.rs") || name.startsWith("testkit") || parts.slice(0, -1).includes("tests");
}
