import { useRef } from "react";
import type { DragEvent, ReactElement } from "react";
import type { Connection } from "../state/connection.ts";
import type { SessionView } from "../state/store.ts";
import { ChatHeader } from "./chat/ChatHeader.tsx";
import { Footer } from "./chat/Footer.tsx";
import { Transcript } from "./chat/Transcript.tsx";
import { buildPrompt, filesFromDrop } from "./composer/attachments.ts";
import type { FileReaderBoundary } from "./composer/attachments.ts";
import { Composer } from "./composer/Composer.tsx";
import type { ComposerHandle } from "./composer/Composer.tsx";
import { TierSelector } from "./composer/TierSelector.tsx";
import { AskCard } from "./dialogs/AskCard.tsx";
import { PlanReviewCard } from "./dialogs/PlanReviewCard.tsx";

const hasFiles = (e: DragEvent): boolean => Array.from(e.dataTransfer?.types ?? []).includes("Files");

export function ChatPane(props: { view: SessionView; connection: Connection; reader: FileReaderBoundary }): ReactElement {
  const { view, connection, reader } = props;
  const composer = useRef<ComposerHandle>(null);
  const { dialog } = view;
  return (
    <div
      data-chat-dropzone
      onDragOver={(e) => { if (hasFiles(e)) e.preventDefault(); }}
      onDrop={(e) => {
        if (!hasFiles(e)) return;
        e.preventDefault();
        void composer.current?.addFiles(filesFromDrop(e.dataTransfer));
      }}
    >
      <ChatHeader sessionKey={view.ref?.key ?? view.id} title={view.title} />
      <Transcript
        items={view.transcript.items}
        hasEarlier={view.transcript.hasEarlier}
        queued={view.queued}
        pending={view.pending}
        onLoadEarlier={() => connection.loadEarlier()}
        tailEpoch={view.transcript.tailEpoch}
      />
      {dialog === null ? null : dialog.method === "ask_user_question" ? (
        <AskCard dialog={dialog} readOnly={!dialog.interactive} onRespond={(r) => connection.respond(dialog.id, r)} />
      ) : (
        <PlanReviewCard dialog={dialog} readOnly={!dialog.interactive} onRespond={(r) => connection.respond(dialog.id, r)} />
      )}
      <TierSelector
        capabilities={view.capabilities}
        footer={view.footer}
        models={view.models}
        busy={view.busy}
        onConfigure={(c) => connection.configure(c)}
        onRequestModels={() => connection.listModels()}
      />
      <Footer footer={view.footer} />
      <Composer
        ref={composer}
        capabilities={view.capabilities}
        busy={view.busy}
        queued={view.queued}
        reader={reader}
        onSubmit={({ text, attachments, whenBusy }) => {
          const { text: prompt, images } = buildPrompt(text, attachments);
          connection.submit({ text: prompt, images, whenBusy });
        }}
        onAbort={() => connection.abort()}
        onCancelQueued={(id) => connection.abortSubmission(id)}
      />
    </div>
  );
}
