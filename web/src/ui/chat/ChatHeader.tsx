import { useEffect } from "react";
import type { JSX } from "react";

export function ChatHeader(props: { sessionKey: string; title: string | null }): JSX.Element {
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
