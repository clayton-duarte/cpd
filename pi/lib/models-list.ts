// The model-discovery helper (docs/contract.md §3.7 E14; docs/commands.md §14.4). STUB: the behaviour comes with the
// implementing Job; `pi/test/models-list.test.ts` pins it.
//
//   node pi/lib/models-list.ts        (no argument; pi's Node; the engine's environment)
//
// The runtime is injected, so a test needs no login: `rt` is what pi's `ModelRuntime` offers. Built-in providers have
// a `refreshModels`; a `models.json` provider has none (docs/research/models-discovery.md D1, D2).

/** What the helper uses of pi's `ModelRuntime` (the public methods of `model-runtime.d.ts`). */
export interface ListRuntime {
  getProviders(): readonly { id: string; baseUrl?: string; refreshModels?: unknown }[];
  /** The key pi resolved from the provider's own `apiKey` setting; throws or answers nothing when it is unset. */
  getAuth(providerId: string): Promise<{ auth: { apiKey?: string } } | undefined>;
  getAllModels(providerId: string): readonly { id: string }[];
  getAvailable(providerId: string): Promise<readonly { id: string }[]>;
  checkAuth(providerId: string): Promise<{ type: "api_key" | "oauth"; source?: string } | undefined>;
  refresh(options: { allowNetwork: boolean; force: boolean; providers: string[] }): Promise<{ aborted: boolean; errors: ReadonlyMap<string, Error> }>;
}

export interface ListDeps {
  rt: ListRuntime;
  piVersion: string;
  /** The `fetch` of a custom provider's list; Node's global one by default. */
  fetch?: typeof fetch;
  /** The timeout of one fetch; 60 000 by default. */
  fetchTimeoutMs?: number;
}

export type Failure = { code: string; message: string; details: Record<string, unknown> };
export type Entry =
  | { provider: string; auth: "oauth" | "api_key" | "custom"; base: string | null; ids: string[]; known?: string[] }
  | { provider: string; error: Failure };
export type Answer = { ok: true; pi_version: string; providers: Entry[] } | { ok: false; error: Failure };

/** Lists every provider pi can use. */
export async function listModels(_deps: ListDeps): Promise<Answer> {
  throw new Error("listModels is not implemented");
}

/** Prints ONE JSON value (the [`Answer`]) and a newline on `stdout`, progress on `stderr`; returns the exit status. */
export async function main(_deps: ListDeps & { stdout: { write(text: string): unknown }; stderr: { write(text: string): unknown } }): Promise<number> {
  throw new Error("main is not implemented");
}
