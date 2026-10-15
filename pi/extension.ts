import { randomUUID } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, watch, writeFileSync, type FSWatcher } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { leadQueueDir, leadWorkflowPath } from "../src/lib/paths.ts";
import { listOpenQuestions } from "../src/queue/escalation-files.ts";
import { adapterActive } from "./lib/activation.ts";
import { createBlockedCounter } from "./lib/blocked.ts";
import { callCpd } from "./lib/cpd.ts";
import { actionOf, errorDelivery } from "./lib/delivery.ts";
import { createReviews, decisionText, lostText, parseRequested, parseStatus, ResultEvent, REVIEW_ENTRY_TYPE, reviewsFromEntries, type Review } from "./lib/plan-review.ts";
import { drain, type Sink } from "./lib/queue.ts";

/** Contract §15: the only pi-specific code. Thin glue; the logic is in ./lib and unit-tested without pi. */

const REQUEST_TIMEOUT_MS = 15_000;
const POLL_MS = 1_000;

type Active = {
  watcher?: FSWatcher;
  timers: ReturnType<typeof setTimeout>[];
  unsubscribe: (() => void)[];
};

export default function (pi: ExtensionAPI) {
  // The session belongs to cpd 2, whose own adapter serves it.
  if (process.env.CPD_SESSION) return;
  if (!adapterActive(process.env)) return;

  let active: Active | undefined;
  let sessionId = "";
  let reviews = createReviews();
  let toolRegistered = false;
  let notify: (message: string) => void = () => {};

  const exec = async (command: string, args: string[]) => {
    const r = await pi.exec(command, args);
    return { stdout: r.stdout, stderr: r.stderr, code: r.code };
  };
  const cpd = (args: string[]) => callCpd(exec, args);
  const report = (what: string, e: unknown) => {
    const d = errorDelivery(`cpd adapter: ${what}: ${e instanceof Error ? e.message : String(e)}`);
    pi.sendMessage(d.message, d.options);
  };

  const attachedWf = (): string | null => {
    try {
      return readFileSync(leadWorkflowPath(sessionId), "utf8").trim() || null;
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
      throw e;
    }
  };

  // --- plan review (contract §15, trial t4) ---

  const request = (action: string, payload: Record<string, unknown>) =>
    new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`plannotator did not answer ${action} (is @plannotator/pi-extension loaded?)`)), REQUEST_TIMEOUT_MS);
      pi.events.emit("plannotator:request", {
        requestId: randomUUID(),
        action,
        payload,
        respond: (r: unknown) => {
          clearTimeout(timer);
          resolve(r);
        },
      });
    });

  const persist = (r: Review) => pi.appendEntry(REVIEW_ENTRY_TYPE, r);

  const tell = (content: string, details: Record<string, unknown>) =>
    pi.sendMessage({ customType: "cpd-plan-decision", content, display: true, details }, { triggerTurn: true, deliverAs: "followUp" });

  /** Record a decision with cpd, then tell the Lead. A duplicate result or poll settles nothing. */
  const decide = async (reviewId: string, approved: boolean, feedback: string | undefined) => {
    const review = reviews.settle(reviewId, approved);
    if (!review) return;
    persist(review);
    let failure: string | undefined;
    try {
      const args = ["plan", "decision", "--review-id", reviewId, approved ? "--approved" : "--rejected", "--wf", review.wf];
      if (feedback !== undefined) {
        const file = join(mkdtempSync(join(tmpdir(), "cpd-feedback-")), "feedback.md");
        writeFileSync(file, feedback);
        args.push("--feedback-file", file);
      }
      await cpd(args);
    } catch (e) {
      failure = e instanceof Error ? e.message : String(e);
    }
    tell(decisionText(review, feedback, failure), { reviewId, wf: review.wf, approved, ...(feedback !== undefined ? { feedback } : {}), ...(failure !== undefined ? { failure } : {}) });
  };

  const pollRecovered = async (ids: string[]) => {
    for (const reviewId of ids) {
      if (reviews.get(reviewId)?.status !== "pending") continue;
      const s = parseStatus(await request("review-status", { reviewId }));
      if (s.status === "completed") await decide(reviewId, s.approved, s.feedback);
      else if (s.status === "missing") {
        const lost = reviews.lose(reviewId);
        if (lost) {
          persist(lost);
          tell(lostText(lost), { reviewId, wf: lost.wf, lost: true });
        }
      }
    }
  };

  const registerTool = () => {
    if (toolRegistered) return;
    toolRegistered = true;
    pi.registerTool({
      name: "cpd_plan_review",
      label: "cpd plan review",
      description:
        "Ask the User to review and approve the current cpd Plan (the draft Jobs of the attached Workflow) in plannotator. Returns at once; the decision arrives later as a cpd message. Call it once per revision.",
      parameters: Type.Object({}),
      exposure: "model-only",
      async execute() {
        const wf = attachedWf();
        if (wf === null) throw new Error("this Lead session is not attached to a Workflow; run: cpd lead attach --wf <wf>");
        const out = join(mkdtempSync(join(tmpdir(), "cpd-plan-")), "plan.md");
        const rendered = await cpd(["plan", "render", "--out", out, "--wf", wf]);
        const planFilePath = typeof rendered.path === "string" ? rendered.path : out;
        const planContent = readFileSync(planFilePath, "utf8");
        const reviewId = parseRequested(await request("plan-review", { planContent, planFilePath }));
        persist(reviews.markPending(reviewId, wf));
        await cpd(["plan", "decision", "--review-id", reviewId, "--pending", "--wf", wf]);
        return {
          content: [{ type: "text" as const, text: `Plan review ${reviewId} requested for workflow ${wf}. The User decides in plannotator; the decision arrives as a cpd message. Stop and wait for it.` }],
          details: { reviewId, wf },
        };
      },
    });
  };

  // --- queue and blocked state (contract §10, §15) ---

  const startQueue = (a: Active, wf: string | null) => {
    const queueDir = leadQueueDir(sessionId);
    mkdirSync(queueDir, { recursive: true });
    const blocked = createBlockedCounter();
    const emit = (ev: ReturnType<typeof blocked.seed>[number] | null) => ev && pi.events.emit("herdr:blocked", ev);
    const sink: Sink = {
      message(msg, delivery) {
        emit(blocked.onMessage(msg));
        const action = actionOf(msg);
        if (action) {
          pi.setSessionName(action.name);
          notify(action.notice);
        }
        if (delivery) pi.sendMessage(delivery.message, delivery.options);
      },
      rejected: (d) => pi.sendMessage(d.message, d.options),
    };
    const safeDrain = () => {
      try {
        drain(queueDir, sink);
      } catch (e) {
        report("queue delivery failed", e);
      }
    };

    if (wf !== null) blocked.seed(listOpenQuestions(wf).map((q) => q.id)).forEach(emit);
    safeDrain();
    a.watcher = watch(queueDir, safeDrain);
    a.watcher.on("error", (e) => report("queue watcher failed", e));
    safeDrain(); // a file that landed between the first drain and the watch
  };

  // --- lifecycle ---

  pi.on("session_start", (_event, ctx) => {
    if (ctx.mode !== "tui") return;
    sessionId = ctx.sessionManager.getSessionId();
    notify = (m) => ctx.ui.notify(m, "info");
    const a: Active = { timers: [], unsubscribe: [] };
    active = a;
    registerTool();

    reviews = createReviews(reviewsFromEntries(ctx.sessionManager.getEntries()));
    a.unsubscribe.push(
      pi.events.on("plannotator:review-result", (data) => {
        const e = ResultEvent.safeParse(data);
        if (e.success && active === a) void decide(e.data.reviewId, e.data.approved, e.data.feedback).catch((err) => report("plan decision failed", err));
      }),
    );

    // herdr-agent-state counts `herdr:blocked` only after its own session_start has run. Handlers run in
    // sequence, so deferring one macrotask puts our first events after it, whatever the load order.
    a.timers.push(
      setTimeout(() => {
        if (active !== a) return;
        try {
          startQueue(a, attachedWf());
        } catch (e) {
          report("cannot start the queue", e);
        }
        const recovered = reviews.pending().map((r) => r.reviewId);
        if (recovered.length > 0) {
          let busy = false;
          const poll = setInterval(() => {
            if (busy || active !== a) return;
            busy = true;
            pollRecovered(recovered)
              .catch((e) => report("plan review poll failed", e))
              .finally(() => {
                busy = false;
                if (!recovered.some((id) => reviews.get(id)?.status === "pending")) clearInterval(poll);
              });
          }, POLL_MS);
          a.timers.push(poll);
        }
      }, 0),
    );
  });

  pi.on("session_shutdown", () => {
    const a = active;
    active = undefined;
    if (!a) return;
    a.watcher?.close();
    a.timers.forEach((t) => {
      clearTimeout(t);
      clearInterval(t);
    });
    a.unsubscribe.forEach((u) => u());
  });
}
