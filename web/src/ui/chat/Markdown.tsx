import { memo } from "react";
import type { ReactElement } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

const FENCE = /^ {0,3}(`{3,}|~{3,})/;

export function splitMarkdown(text: string): string[] {
  const segments: string[] = [];
  let cur: string[] = [];
  let fence: string | null = null;
  let blank = false; // a blank line is pending between segments
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
    } else if (m !== null && m[1]![0] === fence[0] && m[1]!.length >= fence.length && line.trim() === m[1]) {
      fence = null;
    }
    cur.push(line);
  }
  flush();
  return segments;
}

const Segment = memo(function Segment(props: { text: string }): ReactElement {
  return <ReactMarkdown remarkPlugins={[remarkGfm]}>{props.text}</ReactMarkdown>;
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
