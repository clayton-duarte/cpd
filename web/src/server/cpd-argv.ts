// The shared cpd command builder (docs/contract.md §5.6, the web-to-CLI argv contract):
// every cpd argv the web sends goes through here, so one test can prove the whole surface
// parses against the real CLI grammar. This is the argv-tests stub: the union carries a
// single placeholder variant and `allCpdCommands` returns nothing; the argv-impl Jobs fill
// the union with the real commands and route every spawn through `toArgv`.

/** One cpd command the web sends; a discriminated union (docs/contract.md §2.1 conventions). */
export type CpdCommand = { readonly kind: "unimplemented" };

/** The argv after the program name `cpd` (so `"inbox"` is its first element, not `"cpd"`). */
export function toArgv(cmd: CpdCommand): string[] {
  throw new Error(`toArgv is not implemented for ${cmd.kind}`);
}

/** Every command the web can send, for the argv contract test. */
export function allCpdCommands(): CpdCommand[] {
  return [];
}
