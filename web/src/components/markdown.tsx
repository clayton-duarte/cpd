// Markdown rendering for a Chat turn: `react-markdown` for the text. The GFM mapping a turn needs
// — tables in their own scroller, task lists, strikethrough, bare autolinks — and the fenced
// block's Code Block are not here yet: this is the pre-mapping state the red tests pin
// (web/README.md, "The markdown a turn renders"). The impl Job restores the mapping.
import ReactMarkdown from "react-markdown";
import { cn } from "../lib/utils";

/** Renders one markdown run (a user's words or an assistant's answer). */
export function Markdown({
  children,
  className,
}: {
  children: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex min-w-0 flex-col gap-3 text-sm leading-relaxed [&>*:first-child]:mt-0 [&>*:last-child]:mb-0",
        className
      )}
    >
      <ReactMarkdown>{children}</ReactMarkdown>
    </div>
  );
}
