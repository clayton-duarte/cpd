import { SearchIcon } from "lucide-react";
import { Fragment, useEffect } from "react";
import type { ReactElement } from "react";
import { cn } from "../../lib/utils.ts";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
} from "../components/breadcrumb.tsx";
import { glyphClass } from "../dango-status.ts";
import type { Glyph } from "../dango-status.ts";

export interface BreadcrumbCrumb {
  readonly part: "host" | "group" | "manager" | "lead" | "worker";
  readonly label: string;
  readonly onClick?: () => void;
}

export interface HeaderProps {
  readonly crumbs?: readonly BreadcrumbCrumb[];
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
  const { crumbs, location = null, engine = null, onOpenSearch } = props;
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

  const items: readonly BreadcrumbCrumb[] =
    crumbs ??
    (location === null
      ? []
      : [
          { part: "manager", label: location.context },
          ...(location.plan === null ? [] : [{ part: "lead" as const, label: location.plan }]),
        ]);

  return (
    <div data-part="header-bar" className="flex h-full items-center gap-4 border-b border-border px-[14px]">
      <a data-part="home" href="#" aria-label="cpd home" className="flex items-center no-underline">
        <span data-emblem className="sr-only">C-P-D</span>
        <span data-part="phi" aria-hidden="true" className="font-display text-[28px] font-bold leading-none text-fg-strong">φ</span>
      </a>
      {items.length === 0 ? null : (
        <Breadcrumb aria-label="Location" data-part="breadcrumb" className="font-mono text-xs">
          <BreadcrumbList>
            {items.map((crumb, i) => {
              const isLast = i === items.length - 1;
              const partName =
                crumb.part === "manager"
                  ? "breadcrumb-context"
                  : crumb.part === "lead"
                    ? "breadcrumb-plan"
                    : `breadcrumb-${crumb.part}`;
              return (
                <Fragment key={`${crumb.part}:${crumb.label}`}>
                  <BreadcrumbItem>
                    {i > 0 ? <span data-slot="breadcrumb-separator" aria-hidden="true" className="text-muted-foreground">/</span> : null}
                    {crumb.onClick !== undefined ? (
                      <BreadcrumbLink
                        render={<button type="button" />}
                        data-part={partName}
                        aria-current={isLast ? "page" : undefined}
                        onClick={crumb.onClick}
                        className={cn(
                          "cursor-pointer font-mono text-xs",
                          isLast ? "font-medium text-fg-strong" : "text-muted-foreground",
                        )}
                      >
                        {crumb.label}
                      </BreadcrumbLink>
                    ) : isLast ? (
                      <BreadcrumbPage data-part={partName} className="font-mono text-xs text-fg-strong">
                        {crumb.label}
                      </BreadcrumbPage>
                    ) : (
                      <span data-part={partName} className="font-mono text-xs text-muted-foreground">
                        {crumb.label}
                      </span>
                    )}
                  </BreadcrumbItem>
                </Fragment>
              );
            })}
          </BreadcrumbList>
        </Breadcrumb>
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
