import { memo } from "react";
import type { ReactElement } from "react";
import ReactMarkdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  CodeBlock,
  CodeBlockActions,
  CodeBlockContent,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from "../components/code-block.tsx";

const FENCE = /^ {0,3}(`{3,}|~{3,})/;

export function splitMarkdown(text: string): string[] {
  const segments: string[] = [];
  let cur: string[] = [];
  let fence: string | null = null;
  let blank = false;
  const flush = (): void => {
    if (cur.length > 0) segments.push(cur.join("\n"));
    cur = [];
  };
  for (const line of text.split("\n")) {
    if (fence === null && line.trim() === "") {
      blank = true;
      continue;
    }
    if (blank) {
      flush();
      blank = false;
    }
    const m = FENCE.exec(line);
    if (fence === null) {
      if (m !== null) fence = m[1]!;
    } else if (
      m !== null &&
      m[1]![0] === fence[0] &&
      m[1]!.length >= fence.length &&
      line.trim() === m[1]
    ) {
      fence = null;
    }
    cur.push(line);
  }
  flush();
  return segments;
}

const markdownComponents: Components = {
  pre({ children }) {
    return <>{children}</>;
  },
  code({ className, children, ...props }) {
    const raw = String(children ?? "").replace(/\n$/, "");
    const match = /language-([^\s]+)/.exec(className ?? "");
    const isBlock = match !== null || raw.includes("\n");
    if (!isBlock) {
      return (
        <code className={className} {...props}>
          {children}
        </code>
      );
    }
    const lang = match?.[1];
    return (
      <CodeBlock size="sm" className="my-2 not-prose">
        {lang ? (
          <CodeBlockHeader>
            <CodeBlockTitle className="font-mono">{lang}</CodeBlockTitle>
            <CodeBlockActions>
              <CodeBlockCopyButton value={raw} />
            </CodeBlockActions>
          </CodeBlockHeader>
        ) : (
          <CodeBlockActions>
            <CodeBlockCopyButton value={raw} />
          </CodeBlockActions>
        )}
        <CodeBlockContent lineNumbers>{raw}</CodeBlockContent>
      </CodeBlock>
    );
  },
};

const Segment = memo(function Segment(props: { text: string }): ReactElement {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={markdownComponents}
    >
      {props.text}
    </ReactMarkdown>
  );
});

export function Markdown(props: { text: string }): ReactElement | null {
  const segments = splitMarkdown(props.text);
  if (segments.length === 0) return null;
  return (
    <div className="typeset typeset-chat">
      {segments.map((s, i) => (
        <Segment key={i} text={s} />
      ))}
    </div>
  );
}
