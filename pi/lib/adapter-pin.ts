/** A pin: `<share>/pi.pin.<pid>`, a symlink to the bare name of the version directory the process loaded the adapter from (contract, the pin). Returns its path, or null when `adapterDir` is not a version directory. */
export function pinAdapter(adapterDir: string, pid: number = process.pid): string | null {
  void adapterDir;
  void pid;
  throw new Error("not implemented");
}
