// The embedding helper (stub): reads text on stdin, POSTs it to pi's `bifrost` provider, prints one JSON line.
// `pi/test/embed.test.ts` pins it. Not implemented yet.

export interface EmbedRuntime {
  getProviders(): readonly { id: string; baseUrl?: string }[];
  getAuth(providerId: string): Promise<{ auth: { apiKey?: string } } | undefined>;
}

export interface EmbedDeps {
  rt: EmbedRuntime;
  fetch: typeof fetch;
  /** The whole of stdin. */
  stdin: string;
  stdout: { write(text: string): unknown };
  stderr: { write(text: string): unknown };
  /** The vector length asked for and required back. */
  dims: number;
  /** The bound of the request, in ms; the signal comes from `timeoutSignal(timeoutMs)`. */
  timeoutMs: number;
  timeoutSignal: (ms: number) => AbortSignal;
}

/** Prints ONE JSON line; returns the exit status. */
export async function main(_deps: EmbedDeps): Promise<number> {
  throw new Error("not implemented");
}
