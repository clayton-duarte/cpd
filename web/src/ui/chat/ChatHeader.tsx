import { useEffect } from "react";
import type { JSX } from "react";
import type { Runner, SessionRole } from "../../protocol/domain.ts";
import { Button } from "../components/button.tsx";
import { glyphClass } from "../dango-status.ts";

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

const toggleClass = "h-[26px] gap-1.5 rounded-md border-input px-2 text-[11.5px] text-fg-strong";

export function ChatHeader(props: ChatHeaderProps): JSX.Element {
  const { sessionKey, title, role, runner, busy, onToggleThinking, onToggleTools } = props;
  const thinking = props.thinking ?? "folded";
  const tools = props.tools ?? "grouped";
  useEffect(() => {
    const before = document.title;
    document.title = title === null ? sessionKey : `${sessionKey} · ${title}`;
    return () => {
      document.title = before;
    };
  }, [sessionKey, title]);
  return (
    <header className="flex flex-wrap items-center gap-3 px-4 pt-1 pb-2">
      <div data-part="heading" className="min-w-[260px] flex-1">
        {title === null ? null : (
          <span data-part="title" className="block text-title font-semibold text-fg-strong">{title}</span>
        )}
        <div data-part="subtitle" className="mt-[3px] flex items-center gap-2 text-[11.5px]">
          <h1 data-part="key" className="m-0 font-mono text-[11.5px] font-normal text-ramune-bright">{sessionKey}</h1>
          {role === undefined && runner === undefined ? null : <span aria-hidden="true" className="text-fg-dim">{" · "}</span>}
          {role === undefined ? null : <span data-part="role" className="text-fg-dim capitalize">{role}</span>}
          {role !== undefined && runner !== undefined ? <span aria-hidden="true" className="text-fg-dim">{" · "}</span> : null}
          {runner === undefined ? null : <span data-part="runner" className="text-fg-dim">{runner}</span>}
          {busy === true ? (
            <span data-part="run-status" data-state="running" className="flex items-center gap-1.5 text-mizu-bright">
              <span aria-hidden="true" className={glyphClass.active} />
              <span>Running…</span>
            </span>
          ) : null}
        </div>
      </div>
      {onToggleThinking === undefined && onToggleTools === undefined ? null : (
        <div data-part="toggles" role="group" aria-label="Transcript display" className="flex gap-1.5 text-[11.5px]">
          {onToggleThinking === undefined ? null : (
            <Button
              variant="outline"
              type="button"
              data-action="toggle-thinking"
              aria-pressed={thinking === "expanded"}
              aria-keyshortcuts="Control+T"
              className={toggleClass}
              onClick={onToggleThinking}
            >
              <span>{`Thinking: ${thinking}`}</span>
              <kbd className="font-mono text-meta text-muted-foreground">⌃T</kbd>
            </Button>
          )}
          {onToggleTools === undefined ? null : (
            <Button
              variant="outline"
              type="button"
              data-action="toggle-tools"
              aria-pressed={tools === "expanded"}
              aria-keyshortcuts="Control+O"
              className={toggleClass}
              onClick={onToggleTools}
            >
              <span>{`Tools: ${tools}`}</span>
              <kbd className="font-mono text-meta text-muted-foreground">⌃O</kbd>
            </Button>
          )}
        </div>
      )}
    </header>
  );
}
