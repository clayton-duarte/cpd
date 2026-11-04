import { SearchIcon } from "lucide-react";
import { useEffect } from "react";
import type { ReactElement } from "react";
import { glyphClass } from "../dango-status.ts";
import type { Glyph } from "../dango-status.ts";

export interface HeaderProps {
  readonly location?: { readonly context: string; readonly plan: string | null } | null;
  readonly engine?: boolean | null;
  readonly onOpenSearch?: () => void;
}

const ENGINE: Record<"running" | "stopped" | "unknown", { glyph: Glyph; text: string }> = {
  running: { glyph: "done", text: "running" },
  stopped: { glyph: "blocked", text: "not running" },
  unknown: { glyph: "idle", text: "unknown" },
};

export function Header(props: HeaderProps): ReactElement {
  const { location = null, engine = null, onOpenSearch } = props;
  useEffect(() => {
    if (onOpenSearch === undefined) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        onOpenSearch();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onOpenSearch]);
  const state = engine === true ? "running" : engine === false ? "stopped" : "unknown";
  const e = ENGINE[state];
  return (
    <div data-part="header-bar" className="flex h-full items-center gap-4 border-b border-border px-[14px]">
      <a data-part="home" href="#" aria-label="cpd home" className="flex items-center no-underline">
        <span data-emblem className="sr-only">C-P-D</span>
        <span data-part="phi" aria-hidden="true" className="font-display text-[28px] font-bold leading-none text-fg-strong">φ</span>
      </a>
      {location === null ? null : (
        <nav aria-label="Location" data-part="breadcrumb" className="flex items-center gap-1.5 font-mono text-[12px] text-muted-foreground">
          <span data-part="breadcrumb-context" className="text-muted-foreground">{location.context}</span>
          {location.plan === null ? null : (
            <>
              <span aria-hidden="true" className="text-line">/</span>
              <span data-part="breadcrumb-plan" className="text-fg-strong">{location.plan}</span>
            </>
          )}
        </nav>
      )}
      <span className="flex-1" />
      {onOpenSearch === undefined ? null : (
        <>
          <button
            type="button"
            data-part="search"
            onClick={onOpenSearch}
            className="flex h-7 w-[420px] max-w-[40vw] items-center gap-2.5 rounded-[7px] border border-input bg-card px-2.5 text-[12.5px] text-muted-foreground"
          >
            <SearchIcon size={14} aria-hidden="true" />
            <span className="flex-1 text-left">Jump to context, plan, job, PR…</span>
            <kbd className="rounded-[4px] border border-edge px-[5px] font-mono text-meta text-muted-foreground">⌘K</kbd>
          </button>
          <span className="flex-1" />
        </>
      )}
      <span data-part="engine" data-state={state} className="flex items-center gap-[7px] text-[12px] text-muted-foreground">
        <span aria-hidden="true" className={glyphClass[e.glyph]} />
        <span className="font-mono">engine</span>
        <span className="text-fg-dim">{e.text}</span>
      </span>
    </div>
  );
}
