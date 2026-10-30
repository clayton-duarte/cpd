/** The explore guard profile (stub): pattern refusals for scout and research Workers. Not an OS sandbox. */

export type ExploreKind = "scout" | "research";

export type Verdict = { block: string } | undefined;

export function exploreKind(_env: Record<string, string | undefined>): ExploreKind | undefined {
  return undefined;
}

export function exploreVerdict(
  _kind: ExploreKind,
  _call: { name: string; arguments: Record<string, unknown> },
  _where: { cwd: string; home: string; plan: string },
  _isInsideRepo: (path: string) => boolean,
): Verdict {
  return undefined;
}
