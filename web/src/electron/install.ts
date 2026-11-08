// Detects a broken installation and reports it as one structured cpd Event (contract §5.2 `error`;
// web/ELECTRON.md §Run, air-d89 point 1). No real install runs here: every boundary is injected, so
// the unit test uses fakes and never touches the filesystem.

/** One broken installation, as a structured cpd Event (the data of a contract §5.2 `error` Event). */
export interface BrokenInstallation {
  readonly source: 'install';
  readonly code: 'app_dir_missing';
  readonly message: string;
  readonly details: { readonly appDir: string };
}

export interface InstallDeps {
  /** The app dir `cpd install` packs the app into (`<home>/.local/share/cpd/app`, air-d89 point 1). */
  readonly appDir: string;
  /** Whether `path` exists; the real one is `node:fs`'s `existsSync`. */
  exists(path: string): boolean;
  /** Reports one structured event; the real one appends it to the app's event sink. */
  report(event: BrokenInstallation): void;
}

/**
 * Detect a broken installation and report it. Returns the reported event, or `null` when the
 * installation is whole. A missing app dir is broken: nothing was installed where `cpd install`
 * should have packed the app.
 */
export function detectBrokenInstall(deps: InstallDeps): BrokenInstallation | null {
  // Stub: the implementing change detects the missing app dir and reports the structured event.
  void deps;
  return null;
}
