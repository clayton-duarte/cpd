import { failure } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(_argv: string[]): Promise<Result> {
  throw failure("not_implemented", "cpd job add is not implemented");
}
