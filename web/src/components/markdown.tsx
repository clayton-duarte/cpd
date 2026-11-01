// The Markdown renderer for the chat reply (web/README.md "The chat surface"): the assistant's
// turn goes through react-markdown (plus remark-gfm), and a fenced code block is routed to the
// vendored AI Elements `CodeBlock` — its language title and copy button replace a bare `<pre>`.
// This is the one non-registry piece the pinned composition adds; the user's turn is plain text
// in a Bubble and never reaches here.
import type { ComponentProps, ReactElement, ReactNode } from "react";
import ReactMarkdown, { type Components, type ExtraProps } from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  CodeBlock,
  CodeBlockActions,
  CodeBlockContent,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from "./ui/code-block";

/** The language and text of a fenced block, read from its `<code class="language-…">` child. */
function fenceOf(children: ReactNode): { language?: string; text: string } | undefined {
  const child = Array.isArray(children) ? children[0] : children;
  if (child === null || typeof child !== "object" || !("props" in child)) return undefined;
  const props = (child as { props?: { className?: unknown; children?: unknown } }).props;
  const className = typeof props?.className === "string" ? props.className : "";
  const match = /language-([\w-]+)/.exec(className);
  const text = typeof props?.children === "string" ? props.children : "";
  return { language: match?.[1], text };
}

/** A fenced code block as a `CodeBlock`; any other `<pre>` falls back to the element. */
function MarkdownPre({ children }: ComponentProps<"pre"> & ExtraProps): ReactElement {
  const fence = fenceOf(children);
  if (fence === undefined) return <pre>{children}</pre>;
  return (
    <CodeBlock>
      <CodeBlockHeader>
        <CodeBlockTitle>{fence.language ?? "code"}</CodeBlockTitle>
        <CodeBlockActions>
          <CodeBlockCopyButton value={fence.text} />
        </CodeBlockActions>
      </CodeBlockHeader>
      <CodeBlockContent>{fence.text}</CodeBlockContent>
    </CodeBlock>
  );
}

const components: Components = { pre: MarkdownPre };

/** Renders one markdown reply. */
export function Markdown({ children }: { children: string }): ReactElement {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
      {children}
    </ReactMarkdown>
  );
}
