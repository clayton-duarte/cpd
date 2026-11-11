import { useEffect, useState } from "react";
import type { TranscriptItem } from "../../protocol/domain.ts";

export interface SessionStreamProps {
  sessionId: string;
  source?: {
    subscribe: (cb: (item: TranscriptItem) => void) => () => void;
  };
}

export function SessionStream(props: SessionStreamProps) {
  const [items, setItems] = useState<TranscriptItem[]>([]);

  useEffect(() => {
    if (!props.source) return;
    const unsub = props.source.subscribe((item) => {
      setItems((prev) => [...prev, item]);
    });
    return unsub;
  }, [props.source, props.sessionId]);

  return (
    <div data-part="session-stream">
      {items.map((item) => {
        if (item.kind === "user") {
          return (
            <div key={item.id} data-kind="user">
              {item.text}
            </div>
          );
        } else if (item.kind === "assistant") {
          return (
            <div key={item.id} data-kind="assistant">
              {item.blocks
                ?.filter((b) => b.type === "text")
                .map((b, idx) => (
                  <span key={idx}>{(b as { type: "text"; text: string }).text}</span>
                ))}
            </div>
          );
        }
        return (
          <div key={item.id} data-kind={item.kind}>
            {item.id}
          </div>
        );
      })}
    </div>
  );
}
