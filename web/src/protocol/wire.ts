// Browser/server wire types (web/CONTRACT.md section 4). Types only; the parsers belong to the hub/state Jobs.
import type { Capabilities, ConfigureChange, Cursor, Dialog, DialogOutcome, DialogResponse, FooterStats, ModelChoice, QueuedSubmission, SessionRef, TranscriptItem, WhenBusy, Window, WireError, WireImage } from "./domain.ts";
import type { ActionOutput, CockpitAction, ContextTreeModel, InboxModel, PlanViewModel } from "./cockpit.ts";


export type ClientMessage =
  | { type: "cockpit.subscribe" } // idempotent; the server answers with cockpit.snapshot (all panes)
  | { type: "cockpit.selectPlan"; planId: string | null } // choose the Plan shown in the Plan pane; null clears it
  | { type: "cockpit.action"; actionId: string; action: CockpitAction } // actionId: client correlation id, non-empty
  | { type: "session.attach"; sessionId: string } // attaches this connection to that session; an already attached session is detached first
  | { type: "session.detach" } // idempotent
  | { type: "session.loadEarlier"; sessionId: string; before: Cursor }
  | { type: "session.submit"; sessionId: string; clientSubmissionId: string; text: string; images: WireImage[]; whenBusy: WhenBusy }
  | { type: "session.abort"; sessionId: string }
  | { type: "session.abortSubmission"; sessionId: string; clientSubmissionId: string }
  | { type: "session.compact"; sessionId: string; instructions?: string }
  | { type: "session.configure"; sessionId: string; change: ConfigureChange }
  | { type: "session.listModels"; sessionId: string }
  | { type: "dialog.respond"; sessionId: string; dialogId: string; response: DialogResponse };


export type CockpitPane = "tree" | "plan" | "inbox";
export type CockpitPaneData =
  | { pane: "tree"; data: ContextTreeModel }
  | { pane: "plan"; data: PlanViewModel | null } // null = no Plan selected
  | { pane: "inbox"; data: InboxModel };

export interface SessionMeta { ref: SessionRef; footer: FooterStats }

export type ServerMessage =
  // cockpit
  | { type: "cockpit.snapshot"; tree: ContextTreeModel; plan: PlanViewModel | null; inbox: InboxModel } // all three panes; sent on subscribe
  | ({ type: "cockpit.patch" } & CockpitPaneData) // replaces ONE pane (an invalidation refresh); "patch" = per-pane replacement
  | { type: "cockpit.error"; error: WireError } // a refresh failed; the previous snapshot stays valid
  | { type: "action.result"; actionId: string; ok: true; output: ActionOutput }
  | { type: "action.result"; actionId: string; ok: false; error: WireError }
  // session lifecycle
  | { type: "session.attached"; sessionId: string; capabilities: Capabilities; title: string | null; meta: SessionMeta }
  | { type: "session.window"; sessionId: string; mode: "tail" | "earlier"; window: Window } // tail replaces the transcript; earlier is prepended
  | { type: "session.append"; sessionId: string; items: TranscriptItem[] } // new items at the end
  | { type: "session.update"; sessionId: string; items: TranscriptItem[] } // full replacement of existing items by id
  | { type: "session.delta"; sessionId: string; itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string } // text appended to blocks[blockIndex] of an assistant item
  | { type: "session.status"; sessionId: string; busy: boolean; queued: QueuedSubmission[] }
  | { type: "session.submitAck"; sessionId: string; clientSubmissionId: string; queued: boolean; submissionId?: number }
  | { type: "session.footer"; sessionId: string; footer: FooterStats }
  | { type: "session.title"; sessionId: string; title: string | null }
  | { type: "session.models"; sessionId: string; models: ModelChoice[] }
  | { type: "session.dialog"; sessionId: string; dialog: Dialog }
  | { type: "session.dialogResolved"; sessionId: string; dialogId: string; outcome: DialogOutcome }
  | { type: "session.error"; sessionId: string | null; clientSubmissionId?: string; error: WireError }; // sessionId null: not tied to a session (invalid_message, session_not_found of an attach)

export type SessionServerMessage = Extract<ServerMessage, { type: `session.${string}` }>; // what a SessionBackend emits

export function parseClientMessage(_raw: unknown): ClientMessage {
  throw new Error("not implemented");
}
