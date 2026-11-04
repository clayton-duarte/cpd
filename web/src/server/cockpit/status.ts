export interface StatusDeps {
  cpd(argv: readonly string[]): Promise<string>;
  tmux(argv: readonly string[]): Promise<string>;
  readFile(path: string): Promise<string | null>;
}
export interface Status {
  readonly runner: string | null;
  readonly preview: string | null;
  readonly incident: { readonly startedAt: string } | null;
  readonly workers: { readonly queued: number; readonly waiting: number; readonly tenants: number; readonly limit: number } | null;
  readonly version: string | null;
  readonly hostAlert: boolean;
}
export async function readStatus(_deps: StatusDeps): Promise<Status> {
  return { runner: null, preview: null, incident: null, workers: null, version: null, hostAlert: false };
}
