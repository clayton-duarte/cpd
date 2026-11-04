import { useEffect } from "react";
import type { JSX } from "react";
import type { Runner, SessionRole } from "../../protocol/domain.ts";

export interface ChatHeaderProps {
  readonly sessionKey: string;
  readonly title: string | null;
  readonly role?: SessionRole;
  readonly runner?: Runner;
  readonly busy?: boolean;
  readonly thinking?: "folded" | "expanded";
  readonly onToggleThinking?: () => void;
  readonly tools?: "grouped" | "expanded";
  readonly onToggleTools?: () => void;
}

export function ChatHeader(props: ChatHeaderProps): JSX.Element {
  const { sessionKey, title } = props;
  useEffect(() => {
    const before = document.title;
    document.title = title === null ? sessionKey : `${sessionKey} · ${title}`;
    return () => {
      document.title = before;
    };
  }, [sessionKey, title]);
  return (
    <header>
      <h1 data-part="key">{sessionKey}</h1>
      {title === null ? null : <span data-part="title">{title}</span>}
    </header>
  );
}
