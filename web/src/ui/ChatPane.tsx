import { useEffect, useRef, useState } from "react";
import type { DragEvent, ReactElement } from "react";
import type { Connection, SubmissionResult } from "../state/connection.ts";
import type { SessionView } from "../state/store.ts";
import { ChatHeader } from "./chat/ChatHeader.tsx";
import { Transcript } from "./chat/Transcript.tsx";
import { buildPrompt, filesFromDrop } from "./composer/attachments.ts";
import type { FileReaderBoundary } from "./composer/attachments.ts";
import { Composer } from "./composer/Composer.tsx";
import type { ComposerHandle } from "./composer/Composer.tsx";
import { TierSelector } from "./composer/TierSelector.tsx";
import { AskCard } from "./dialogs/AskCard.tsx";
import { PlanReviewCard } from "./dialogs/PlanReviewCard.tsx";

function useChatToggles(): {
  thinking: "folded" | "expanded";
  tools: "grouped" | "expanded";
} {
  const [thinking, setThinking] = useState<"folded" | "expanded">("folded");
  const [tools, setTools] = useState<"grouped" | "expanded">("grouped");
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (!e.ctrlKey || e.metaKey || e.altKey || e.shiftKey || e.isComposing) return;
      const k = e.key.toLowerCase();
      if (k === "t") setThinking((v) => (v === "folded" ? "expanded" : "folded"));
      else if (k === "o") setTools((v) => (v === "grouped" ? "expanded" : "grouped"));
      else return;
      e.preventDefault();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  return { thinking, tools };
}

const hasFiles = (e: DragEvent): boolean => Array.from(e.dataTransfer?.types ?? []).includes("Files");

export function ChatPane(props: { view: SessionView; connection: Connection; reader: FileReaderBoundary }): ReactElement {
  const { view, connection, reader } = props;
  const composer = useRef<ComposerHandle>(null);
  const { dialog } = view;
  const toggles = useChatToggles();
  const sessionKey = view.ref?.key ?? view.id;
  const isBusy = view.busy || view.pending.some((p) => p.state === "sending");
  return (
    <div
      data-chat-dropzone
      onDragOver={(e) => { if (hasFiles(e)) e.preventDefault(); }}
      onDrop={(e) => {
        if (!hasFiles(e)) return;
        e.preventDefault();
        void composer.current?.addFiles(filesFromDrop(e.dataTransfer));
        composer.current?.focus();
      }}
    >
      <ChatHeader
        sessionKey={sessionKey}
        title={view.title}
        completion={view.completion ?? null}
        {...(view.ref === null ? {} : { role: view.ref.role, runner: view.ref.runner })}
      />
      <Transcript
        items={view.transcript.items}
        hasEarlier={view.transcript.hasEarlier}
        busy={isBusy}
        queued={view.queued}
        pending={view.pending}
        onLoadEarlier={() => connection.loadEarlier()}
        tailEpoch={view.transcript.tailEpoch}
        sessionKey={sessionKey}
        thinking={toggles.thinking}
        tools={toggles.tools}
      />
      <div hidden={dialog !== null}>
        <Composer
          ref={composer}
          sessionId={view.id}
          sessionKey={sessionKey}
          capabilities={view.capabilities}
          archived={view.archived ?? false}
          busy={isBusy}
          queued={view.queued}
          reader={reader}
          settings={
            <TierSelector
              capabilities={view.capabilities}
              footer={view.footer}
              models={view.models}
              busy={view.busy}
              onConfigure={(c) => connection.configure(c)}
              onRequestModels={() => connection.listModels()}
            />
          }
          onSubmit={({ text, attachments, whenBusy }) => {
            const { text: prompt, images } = buildPrompt(text, attachments);
            const id = connection.submit({ text: prompt, images, whenBusy });
            return (settle: (result: SubmissionResult) => void) => connection.observeSubmission(view.id, id, (result) => {
              settle(view.ref?.runner === "pi" && result.type === "ack" ? { type: "unknown" } : result);
            });
          }}
          onAbort={() => connection.abort()}
          onCancelQueued={(id) => connection.abortSubmission(id)}
        />
      </div>
      {dialog === null ? null : dialog.method === "ask_user_question" ? (
        <AskCard dialog={dialog} readOnly={!dialog.interactive} onRespond={(r) => connection.respond(dialog.id, r)} />
      ) : (
        <PlanReviewCard dialog={dialog} readOnly={!dialog.interactive} onRespond={(r) => connection.respond(dialog.id, r)} />
      )}
    </div>
  );
}
