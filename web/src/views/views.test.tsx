// The cockpit views harness (docs/research/web-client-api.md §5, §6). It renders every view
// on the typed mock read-model/frames and asserts the whole cockpit is no-throw; a second
// test drives Chat with mock frames and asserts the folded transcript renders; a third
// renders a held `ui_request` as an approval dialog.
//
// RED: the view stubs throw `not implemented`, so these three tests fail for the right
// reason until the views-impl Job lands. Rendering is server-side (react-dom/server
// renderToStaticMarkup) in the default node environment: no view needs a browser API, so no
// jsdom/happy-dom is added and web/package.json is untouched.
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import type { ReactElement } from "react";
import { Chat, Inbox, Logs, Plan, Tree, type ChatFrame, type LogEntry } from "./index";
import { ApprovalDialog } from "../app/App";
import type { UiRequest } from "../api/types";
import type {
  InboxListResult,
  PlanListResult,
  PlanShowResult,
  SessionListResult,
} from "../api/readmodel";

// --- the mock data: the read-model slices the views render (§3) -------------------------

const planList: PlanListResult = {
  ok: true,
  plans: [
    {
      id: "plan-1",
      context: "ctx-1",
      title: "web-client",
      phase: "active",
      lead_session: "session-1",
      archived_at: null,
      counts: { draft: 0, queued: 0, running: 1, merged: 0, failed: 0, cancelled: 0 },
      prs: [],
    },
  ],
  events_cursor: "evt-1",
};

const sessions: SessionListResult = {
  ok: true,
  sessions: [
    {
      id: "session-1",
      role: "lead",
      context: "ctx-1",
      plan: "plan-1",
      attempt: null,
      name: "web-client",
      runner: "durable",
      state: "working",
      state_label: "working",
      state_at: null,
      archived_at: null,
      asleep_at: null,
      place: "chat",
      tmux_pane: null,
      file: null,
      forked_from: null,
      plan_lead: null,
      tier: null,
      cwd: "/home/clayton/projects/cpd",
      title: "web-client",
      titled_at: null,
    },
  ],
  events_cursor: "evt-1",
};

const inbox: InboxListResult = {
  ok: true,
  engine: { running: true },
  items: [
    {
      id: "item-1",
      kind: "plan-review",
      urgency: "magenta",
      label: "plan review",
      subject: "web-client",
      context: "ctx-1",
      plan: "plan-1",
      job: null,
      session: "session-1",
      record: null,
      event: null,
      url: null,
      opened_at: null,
      closed_at: null,
      snoozed_until: null,
      dismissed_at: null,
      claimed_at: null,
      age_seconds: 5,
    },
  ],
  hidden: { snoozed: 0, dismissed: 0, claimed: 0 },
  events_cursor: "evt-1",
};

const planShow: PlanShowResult = {
  ok: true,
  plan: {
    id: "plan-1",
    context: "ctx-1",
    title: "web-client",
    jira: null,
    phase: "active",
    lead_session: "session-1",
    created_at: "2026-10-31T21:41:00-04:00",
    locked_at: null,
    done_at: null,
    archived_at: null,
    outline: null,
  },
  prs: [],
  jobs: [],
  escalations: null,
  review: null,
  counts: null,
  tamper: null,
  events_cursor: "evt-1",
};

const logEntries: readonly LogEntry[] = [
  { id: 1, at: "2026-10-31T21:41:00-04:00", kind: "run_start", text: "the run started" },
  { id: 2, at: "2026-10-31T21:41:05-04:00", kind: "error", text: "a test failed" },
];

// A small frame sequence in the shape of durable-leads §4.6.2 (a submit with a short answer:
// a whole message arrives at message_start and is replaced at message_end).
const chatFrames: readonly ChatFrame[] = [
  {
    type: "events",
    epoch: "e-7f3a",
    seq: 6,
    events: [{ kind: "message_start", id: "m1", role: "user", text: "run the checks" }],
  },
  {
    type: "events",
    epoch: "e-7f3a",
    seq: 7,
    events: [
      { kind: "message_start", id: "m2", role: "assistant", text: "ok" },
      { kind: "message_end", id: "m2", text: "ok, the checks pass" },
    ],
  },
];

// The approval `ui_request` of durable-leads §4.5 row 23 (a secret approval, select).
const approval: UiRequest = {
  type: "ui_request",
  id: "b3b2e6f0-2a41-4c0e-9d52-6c1d8a0b7e11",
  method: "select",
  title: "lead-1 asks to get in vault cpd: item gh, field token. Reason: open a PR",
  options: [
    { label: "Allow once", description: "answer once" },
    { label: "Deny", description: "refuse" },
  ],
  purpose: "secret_approve",
  created_at: "2026-10-31T21:41:00-04:00",
  tool_call_id: "call_7",
};

// --- the tests --------------------------------------------------------------------------

test("every cockpit view renders on the mock data without throwing", () => {
  const views: readonly [string, ReactElement][] = [
    ["Tree", <Tree plans={planList} sessions={sessions} inbox={inbox} />],
    ["Plan", <Plan plan={planShow} />],
    ["Inbox", <Inbox inbox={inbox} />],
    ["Logs", <Logs events={logEntries} />],
    ["Chat", <Chat epoch="e-7f3a" frames={chatFrames} />],
  ];
  for (const [name, element] of views) {
    expect(() => renderToStaticMarkup(element), `${name} threw`).not.toThrow();
  }
});

test("the chat view folds the mock frames and renders the transcript", () => {
  const markup = renderToStaticMarkup(<Chat epoch="e-7f3a" frames={chatFrames} />);
  // Both messages of the fold are in the transcript (§5.1, §5.2).
  expect(markup).toContain("run the checks");
  // The whole message is replaced at message_end, not concatenated (§5.2): a concatenation
  // would render "okok, the checks pass", which this exact folded text does not match.
  expect(markup).toContain("ok, the checks pass");
});

test("an approval ui_request renders as a dialog", () => {
  const markup = renderToStaticMarkup(<ApprovalDialog request={approval} />);
  // The dialog shows the request title and every option (§1.2 row 23, hs-d775).
  expect(markup).toContain("vault cpd: item gh, field token");
  expect(markup).toContain("Allow once");
  expect(markup).toContain("Deny");
});
