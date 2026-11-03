// The classifier helper (stub): reads `{state,question,labels}` on stdin, asks pi's classifier, prints one JSON line.
// `pi/test/classify.test.ts` pins it. Not implemented yet.

export interface ClassifyRuntime {
  getModelOfType(type: "classifier", providerId: string, modelId: string): unknown;
  classify(model: unknown, context: unknown, options?: { signal?: AbortSignal }): Promise<unknown>;
}

export interface ClassifyDeps {
  rt: ClassifyRuntime;
  /** The whole of stdin. */
  stdin: string;
  stdout: { write(text: string): unknown };
  stderr: { write(text: string): unknown };
  /** The bound of the request, in ms (3000); the signal comes from `timeoutSignal(timeoutMs)`. */
  timeoutMs: number;
  timeoutSignal: (ms: number) => AbortSignal;
}

/** Prints ONE JSON line; returns the exit status. */
export async function main(_deps: ClassifyDeps): Promise<number> {
  throw new Error("not implemented");
}
