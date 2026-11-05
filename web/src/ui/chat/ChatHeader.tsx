import { useEffect } from "react";
import type { JSX } from "react";
import type { Runner, SessionRole } from "../../protocol/domain.ts";

export interface ChatHeaderProps {
  readonly sessionKey: string;
  readonly title: string | null;
  readonly role?: SessionRole;
  readonly runner?: Runner;
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
    <header className="flex flex-wrap items-center gap-3 border-b px-4 py-2">
      <div data-part="heading" className="min-w-0 flex-1">
        <h1
          data-part="title"
          className="m-0 truncate text-title font-semibold text-fg-strong"
        >
          {title ?? sessionKey}
        </h1>
      </div>
    </header>
  );
}
