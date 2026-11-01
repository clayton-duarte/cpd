// Markdown rendering for a Chat turn: `react-markdown` for the text, with every fenced code block
// rendered through the shadcn Code Block (`ui.shadcn.com/docs/components/base/code-block`) so a
// turn's code, log or diff arrives with a title, a language and a copy action.
//
// `remark-gfm` is what a turn's tables, task lists, strikethrough and bare autolinks need: CommonMark
// alone renders a table as a paragraph of pipes, which is how a Lead's status table reached the Chat.
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "../lib/utils";
import {
  CodeBlock,
  CodeBlockActions,
  CodeBlockContent,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from "./ui/code-block";

/** The language of a fenced block's `className` (`language-ts`), or undefined for a plain fence. */
function fenceLanguage(className: unknown): string | undefined {
  if (typeof className !== "string") return undefined;
  return /language-([\w-]+)/.exec(className)?.[1];
}

/** The text of a `<code>` element's children, whatever React made of them. */
function codeText(children: unknown): string {
  if (typeof children === "string") return children;
  if (typeof children === "number") return String(children);
  if (Array.isArray(children)) return children.map(codeText).join("");
  return "";
}

/** A fenced block becomes a Code Block; inline code stays inline. */
const components: Components = {
  // A table is wide by nature and Tailwind's preflight strips its borders and padding, so it gets
  // a scroller, rules between its rows and room in its cells; a header cell never wraps.
  table({ children }) {
    return (
      <div className="w-full overflow-x-auto">
        <table className="w-full border-collapse text-sm">{children}</table>
      </div>
    );
  },
  th({ children, style }) {
    return (
      <th
        style={style}
        className="border-b border-border px-2 py-1.5 text-start font-medium whitespace-nowrap"
      >
        {children}
      </th>
    );
  },
  td({ children, style }) {
    return (
      <td style={style} className="border-b border-border/60 px-2 py-1.5 align-top">
        {children}
      </td>
    );
  },
  pre({ children }) {
    const child = (Array.isArray(children) ? children[0] : children) as
      | { props?: { className?: unknown; children?: unknown } }
      | undefined;
    const text = codeText(child?.props?.children).replace(/\n$/, "");
    const language = fenceLanguage(child?.props?.className);
    // A fence with no code in it (a stray `pre`) renders as markdown does.
    if (text === "") return <pre>{children}</pre>;
    return (
      <CodeBlock>
        <CodeBlockHeader>
          <CodeBlockTitle>{language ?? "text"}</CodeBlockTitle>
          <CodeBlockActions>
            <CodeBlockCopyButton value={text} />
          </CodeBlockActions>
        </CodeBlockHeader>
        <CodeBlockContent>{text}</CodeBlockContent>
      </CodeBlock>
    );
  },
  a({ className, ...props }) {
    return (
      <a
        className={cn("font-medium underline underline-offset-3", className)}
        target="_blank"
        rel="noreferrer"
        {...props}
      />
    );
  },
};

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
      <ReactMarkdown components={components} remarkPlugins={[remarkGfm]}>
        {children}
      </ReactMarkdown>
    </div>
  );
}
