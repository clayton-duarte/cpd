// The team channel's rules, ported from hermes-config `plugins/cockpit/` (read-only; no GitHub here):
// `refresh.py` (`unwrap`, `_unescape`, `parse_messages`), `classify.py` (`extract_prs`, `parse_reactions`,
// `mentions_me`, `slack_claim`) and `slack_fetch.py` (`reaction_users`). docs/research/team-review.md §6.
// Pure: no I/O, no clock (callers pass `now`), so every rule is a test on a transcript.

export const MESSAGE_HEADER = "=== Message " + "from ";
/** `refresh.RETAIN_DAYS` and `main`'s `--since-hours` default: a post older than this is not read (refresh.py 496, 626-641). */
export const WINDOW_DAYS = 7;
/** `classify.CLAIM_REACTIONS`: a reaction that means "a human has this". */
const CLAIM_REACTIONS = new Set(["eyes"]);

const PR_URL_RE = /github\.com\/([\w.-]+)\/([\w.-]+)\/pull\/(\d+)/g;
const REACTION_RE = /^Reactions: (.+)$/gm;
const MENTION_RE = /<@(U[A-Z0-9]+)\|/g;
/** The app's own footer, `*Sent using* <@U…|Name>`, matched by shape and not by id (classify.py 82-84). */
const APP_ATTRIBUTION_RE = /^[ \t]*\*Sent using\*[ \t]*<@U[A-Z0-9]+\|[^>]*>[ \t]*$/gm;

export type Claim = { kind: "claimed"; reason: string };

export type Post = {
  repo: string;
  number: number;
  /** Slack's timestamp as a number, seconds. */
  ts: number;
  /** Slack's own string, kept exact: the reactions API matches on the literal value (refresh.py 174-176). */
  ts_raw: string;
  author: string;
  /** The post is about this PR, as opposed to merely mentioning it: the first PR URL of the post. */
  primary: boolean;
  reactions: string[];
  claim: Claim | null;
  /** Whether the User's own `:eyes:` is the only claim; null when unread or unattributable. Filled by the reader. */
  eyes_me: boolean | null;
  /** Whether the post's thread mentions the User. Filled by the reader. */
  thread_tags_me: boolean;
};

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** `refresh._unescape`: best-effort decode of a JSON-escaped transcript that will not parse (up to four layers). */
export function unescapeTranscript(text: string): string {
  for (let i = 0; i < 4; i++) {
    if (text.includes("\n" + MESSAGE_HEADER) && text.includes("/pull/")) break;
    text = text
      .replaceAll("\\\\n", "\n")
      .replaceAll("\\n", "\n")
      .replaceAll('\\"', '"')
      .replaceAll("\\\\/", "/")
      .replaceAll("\\/", "/");
  }
  return text;
}

/** `refresh.unwrap`: the transcript out of the tool's envelope, which is sometimes truncated mid-JSON. */
export function unwrap(raw: string): string {
  let payload: unknown;
  try {
    payload = JSON.parse(raw);
  } catch {
    return unescapeTranscript(raw);
  }
  if (!isObject(payload)) return unescapeTranscript(raw);
  const result = "result" in payload ? payload.result : payload;
  if (typeof result === "string") {
    try {
      const inner: unknown = JSON.parse(result);
      if (isObject(inner)) return "messages" in inner ? textOf(inner.messages) : result;
    } catch {
      // not JSON: the text itself, escaped or not
    }
    return unescapeTranscript(result);
  }
  if (isObject(result)) return "messages" in result ? textOf(result.messages) : JSON.stringify(result);
  return String(result);
}

const textOf = (v: unknown): string => (typeof v === "string" ? v : JSON.stringify(v));

/** `classify.extract_prs`: `[owner/repo, number]` for every PR URL, `.,)>|` stripped from the repo. */
export function extractPrs(text: string): Array<[string, number]> {
  return [...text.matchAll(PR_URL_RE)].map(([, owner, repo, n]) => [`${owner}/${repo!.replace(/[.,)>|]+$/, "")}`, Number(n)]);
}

/** `classify.parse_reactions`: the names on `Reactions: eyes (1), github-check-mark (2)`, without counts. */
export function parseReactions(block: string): string[] {
  const names = new Set<string>();
  for (const [, line] of block.matchAll(REACTION_RE)) {
    for (const part of line!.split(",")) {
      const name = part.trim().split(" (")[0]!.trim();
      if (name) names.add(name);
    }
  }
  return [...names];
}

const mentioned = (text: string): Set<string> => new Set([...text.matchAll(MENTION_RE)].map((m) => m[1]!));

/** `classify.mentions_me`: whether `text` names `me` in a real mention (an app signing its own post is not one). */
export function mentionsMe(text: string, me: string): boolean {
  return mentioned(text.replace(APP_ATTRIBUTION_RE, "")).has(me);
}

/** `classify.slack_claim`: attention only. A reaction `eyes` claims; a mention of someone else claims; a mention of `me` does not. */
export function slackClaim(block: string, me: string): Claim | null {
  if (parseReactions(block).some((r) => CLAIM_REACTIONS.has(r))) return { kind: "claimed", reason: "claimed with :eyes: in Slack" };
  const named = mentioned(block.replace(APP_ATTRIBUTION_RE, ""));
  if (named.size > 0) return named.has(me) ? null : { kind: "claimed", reason: "assigned to someone in Slack" };
  return null;
}

/**
 * `refresh.parse_messages`: the in-window PR references, newest first, one per PR (the newest posting wins).
 * `cutoff` is epoch seconds. A block with no `Message TS:` is skipped.
 */
export function parseMessages(text: string, cutoff: number, me: string): Post[] {
  const seen = new Map<string, Post>();
  for (const block of text.split(MESSAGE_HEADER).slice(1)) {
    const tsRaw = /Message TS: ([\d.]+)/.exec(block)?.[1];
    if (tsRaw === undefined) continue;
    const ts = Number.parseFloat(tsRaw);
    if (Number.isNaN(ts) || ts < cutoff) continue;
    const author = block.split("(")[0]!.trim().split("<")[0]!.trim();
    const claim = slackClaim(block, me);
    const found = [...new Map(extractPrs(block).map((pr) => [`${pr[0]}#${pr[1]}`, pr])).values()];
    const primary = found[0];
    for (const [repo, number] of found) {
      const key = `${repo}#${number}`;
      const old = seen.get(key);
      if (old !== undefined && ts <= old.ts) continue;
      seen.set(key, {
        repo,
        number,
        ts,
        ts_raw: tsRaw,
        author,
        primary: primary !== undefined && repo === primary[0] && number === primary[1],
        reactions: parseReactions(block),
        claim,
        eyes_me: null,
        thread_tags_me: false,
      });
    }
  }
  return [...seen.values()].sort((a, b) => b.ts - a.ts);
}

/** How many `=== Message from ` blocks the transcript holds. */
export const countMessages = (text: string): number => text.split(MESSAGE_HEADER).length - 1;

/**
 * `slack_fetch.reaction_users`, by Slack user id (the research's deviation 2): the people on the `:<emoji>:` line of
 * `slack_get_reactions` (`:eyes: × 2 — Ryan Case (U039194B7D2), clayton (U04DB4MTUMR)`, an em dash), each with the id in
 * the parentheses when it has one. The `{"result": "…"}` envelope is unwrapped first.
 */
export function reactionUsers(raw: string, emoji: string): Array<{ name: string; id: string | null }> {
  let text = raw;
  try {
    const payload: unknown = JSON.parse(raw);
    if (isObject(payload) && typeof payload.result === "string") text = payload.result;
  } catch {
    // the raw text, as hermes falls back
  }
  const users: Array<{ name: string; id: string | null }> = [];
  for (const line of text.split(/\r?\n/)) {
    const dash = line.indexOf("\u2014");
    if (dash < 0) continue;
    const head = line.slice(0, dash);
    const tail = line.slice(dash + 1);
    if (!head.includes(`:${emoji}:`) || !tail.trim()) continue;
    for (const entry of tail.split(",")) {
      const name = entry.replace(/\([^)]*\)/g, "").trim();
      if (name) users.push({ name, id: /\((U[A-Z0-9]+)\)/.exec(entry)?.[1] ?? null });
    }
  }
  return users;
}

/** `refresh.claimed_by_me`: true when `me`'s `:eyes:` is the only claim, false when a peer is on it, null when unattributable. */
export function claimedByMe(users: Array<{ id: string | null }>, me: string): boolean | null {
  if (users.length === 0) return null;
  return users.every((u) => u.id === me);
}
