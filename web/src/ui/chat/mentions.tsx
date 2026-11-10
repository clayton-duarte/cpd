import type { ReactNode } from "react";
import { parseAgentKey } from "../agentKey.ts";

export interface MentionLinkProps {
  readonly agentKey: string;
  readonly href: string;
  readonly children: ReactNode;
}

export function MentionLink(props: MentionLinkProps): ReactNode {
  return (
    <a
      data-part="agent-mention"
      data-agent-key={props.agentKey}
      href={props.href}
      className="underline decoration-muted-foreground hover:text-foreground"
    >
      {props.children}
    </a>
  );
}

// Token pattern: maximal run of [A-Za-z0-9:-]
const TOKEN_RE = /[A-Za-z0-9:-]+/g;

export function linkifyMentions(
  text: string,
  liveAgentKeys: ReadonlySet<string>,
): ReactNode {
  if (!text) return text;

  const result: ReactNode[] = [];
  let lastIndex = 0;

  TOKEN_RE.lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = TOKEN_RE.exec(text)) !== null) {
    const candidate = match[0];
    const startIndex = match.index;
    const endIndex = startIndex + candidate.length;

    // Check boundary chars: neither /, \ nor @ adjacent
    const prevChar = startIndex > 0 ? text[startIndex - 1] : "";
    const nextChar = endIndex < text.length ? text[endIndex] : "";

    const isAdjacentForbidden =
      prevChar === "/" ||
      prevChar === "\\" ||
      prevChar === "@" ||
      nextChar === "/" ||
      nextChar === "\\" ||
      nextChar === "@";

    if (!isAdjacentForbidden && parseAgentKey(candidate) && liveAgentKeys.has(candidate)) {
      if (startIndex > lastIndex) {
        result.push(text.slice(lastIndex, startIndex));
      }

      // Convert key to href: cpd -> #/a/cpd, cpd:plan-x -> #/a/cpd/plan-x, cpd:plan-x:web-tests -> #/a/cpd/plan-x/web-tests
      const pathSuffix = candidate.split(":").join("/");
      const href = `#/a/${pathSuffix}`;

      result.push(
        <MentionLink key={`${startIndex}-${candidate}`} agentKey={candidate} href={href}>
          {candidate}
        </MentionLink>,
      );
      lastIndex = endIndex;
    }
  }

  if (lastIndex === 0) {
    return text;
  }

  if (lastIndex < text.length) {
    result.push(text.slice(lastIndex));
  }

  return <>{result}</>;
}
