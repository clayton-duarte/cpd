import { isObject, toCpdError } from "./cpd.ts";
import type { Host } from "./host.ts";
import { boldPhi } from "./phi.ts";
import { isTyped, startsReply } from "./reply.ts";
import { lastAssistantText } from "./reminder.ts";
import type { ModelCtx } from "./pi.ts";
import type { Run } from "./run.ts";

/** commands.md §15.26: the turn-end question guard of a Lead or Manager. */

const ANSWERING_TOOLS = ["ask_user_question", "cpd_plan_review", "cpd_secret_approve", "cpd_host_approve"];
const THRESHOLD = 0.85;
const TIMEOUT_MS = 1500;
const TAIL_CHARS = 2000;
export const NUDGE =
  "You ended your turn with a question in plain text. If you need a decision or input from the User, call `ask_user_question` now so the session enters `blocked` state; otherwise state your assumption in one line and proceed.";
const NOTICE = boldPhi("φ: classifier_unreachable: http://ollama.lab/v1 is unreachable, so the turn-end question guard is skipped (connect Tailscale or turn it off: cpd config experimental turn_question_guard off).");

const guarded = (r: Run) => r.info.role === "lead" || r.info.role === "manager";

/** The last assistant message of the run ended with stopReason "stop"; its text, or undefined. */
function finalText(messages: unknown): string | undefined {
  if (!Array.isArray(messages)) return undefined;
  const last: unknown = messages.findLast((m) => isObject(m) && m.role === "assistant");
  if (!isObject(last) || last.stopReason !== "stop") return undefined;
  return lastAssistantText([last]);
}

type Outcome = { nudge: boolean; reached: boolean | undefined };

/** Records a tool call of the run, nested ones too. Called by the adapter's one `tool_call` handler: a handler of its own would come before it and hide its verdict from `fireWith`'s first result. */
export function noteQuestionTool(host: Host, toolName: string): void {
  const r = host.run;
  if (r && guarded(r)) r.questionTools.add(toolName);
}

/** Registered once (pi has no unsubscribe); the run and its role are read at each call. */
export function registerQuestionGuard(host: Host): void {
  const newRun = (event: unknown) => {
    const r = host.run;
    if (!r || !guarded(r)) return;
    r.questionTools.clear();
    r.questionCandidate = undefined;
    // pi starts the run of `sendUserMessage` with messageType "user" too: only a reply start that is not that resets the count.
    if (startsReply(event) && isObject(event) && event.messageType !== "user") r.questionNudges = 0;
  };
  // Named like the loop guard's observer: it adds no message, and the test fake's `fireWith` leaves handlers of that name out of its list (pi/test/fake.ts).
  host.pi.on("before_agent_start", function loopGuardHandler(event) {
    newRun(event);
  });
  host.pi.on("agent_start", () => {
    const r = host.run;
    if (r && guarded(r)) r.questionCandidate = undefined;
  });
  host.pi.on("input", (event) => {
    const r = host.run;
    if (r && guarded(r) && isTyped(event)) r.questionNudges = 0;
  });
  host.pi.on("agent_end", (event) => {
    const r = host.run;
    if (!r || !guarded(r)) return;
    const called = ANSWERING_TOOLS.some((t) => r.questionTools.has(t));
    r.questionTools.clear();
    const text = finalText(event?.messages);
    r.questionCandidate = text !== undefined && text.includes("?") && !called ? { text } : undefined;
  });
  host.pi.on("agent_settled", (_event, ctx) => {
    const r = host.run;
    if (!r || !guarded(r) || r.questionCandidate === undefined || r.questionNudges >= 1) return;
    const candidate = r.questionCandidate;
    const current = () => host.run === r && r.questionCandidate === candidate && !r.abort.signal.aborted;
    void (async () => {
      try {
        if (!(await enabled(host, r)) || !current()) return;
        if (r.info.role === "lead" && r.info.plan !== null && !(await idlePlan(host, r.info.plan)) ) return;
        if (!current()) return;
        const model = ctx.modelRegistry.findOfType("classifier", "llama.cpp", "kev-4b");
        if (model === undefined) return;
        const out = await classify(ctx as ModelCtx, r, model, candidate.text);
        if (host.run !== r) return;
        if (out.reached !== undefined) await recordHealth(host, r, out.reached);
        if (host.run !== r) return;
        if (out.reached === true) r.classifierDown = false;
        else if (out.reached === false && !r.classifierDown) {
          r.classifierDown = true;
          ctx.ui.notify(NOTICE, "warning");
        }
        if (!out.nudge || !current() || r.tripped || r.firstPending || r.questionNudges >= 1) return;
        r.questionCandidate = undefined;
        host.pi.sendUserMessage(NUDGE, { deliverAs: "steer" });
        r.questionNudges += 1;
      } catch (e) {
        r.log(`question guard: ${toCpdError(e).message}`);
      }
    })();
  });
}

/** Records the classifier call on `swr_endpoints('ollama.lab')`; a failed record never fails the guard. */
async function recordHealth(host: Host, r: Run, reached: boolean): Promise<void> {
  try {
    const tail = reached ? ["ok"] : ["error", "--text", "ollama.lab is unreachable"];
    await host.call(["swr", "health", "record", "classifier", "ollama.lab", ...tail]);
  } catch (e) {
    r.log(`question guard: cpd swr health record failed: ${toCpdError(e).message}`);
  }
}

/** Q5: `cpd config`, read now; on unless `experimental.turn_question_guard` is false. A failed read does nothing. */
async function enabled(host: Host, r: Run): Promise<boolean> {
  try {
    const config = (await host.call(["config"])).config;
    const experimental = isObject(config) ? config.experimental : undefined;
    return !(isObject(experimental) && experimental.turn_question_guard === false);
  } catch (e) {
    r.log(`question guard: cpd config failed: ${toCpdError(e).message}`);
    return false;
  }
}

/** Q4 (2): the Plan has no running or queued Job. */
async function idlePlan(host: Host, plan: string): Promise<boolean> {
  const counts = (await host.call(["plan", "show", "--plan", plan])).counts;
  return isObject(counts) && counts.running === 0 && counts.queued === 0;
}

type Ctx = ModelCtx;

/** Q6: one bounded classifier call; `reached` is true (answered), false (unreachable) or undefined (neither). */
async function classify(ctx: Ctx, r: Run, model: Parameters<Ctx["modelRegistry"]["classify"]>[0], text: string): Promise<Outcome> {
  const none: Outcome = { nudge: false, reached: undefined };
  try {
    const res = await ctx.modelRegistry.classify(
      model,
      {
        state: { message: text.slice(-TAIL_CHARS) },
        questions: {
          asking_user: {
            type: "bool",
            instructions: "Does this assistant message ask the human user a question or ask for a decision/confirmation before proceeding?",
            criteria: {
              true: "The assistant is asking the user a question, offering choices, or waiting for user confirmation/input.",
              false: "The assistant is only reporting status, summarizing completed work, or rhetorical/quoted question marks.",
            },
          },
        },
      },
      { signal: AbortSignal.any([r.abort.signal, AbortSignal.timeout(TIMEOUT_MS)]), maxRetries: 0 },
    );
    if (res.stopReason === "stop") {
      const a = res.answers?.asking_user;
      return { nudge: isObject(a) && a.type === "bool" && typeof a.probability === "number" && a.probability >= THRESHOLD, reached: true };
    }
    if (res.stopReason === "error" || (res.stopReason === "aborted" && !r.abort.signal.aborted)) return { nudge: false, reached: false };
    return none;
  } catch (e) {
    r.log(`question guard: classifier failed: ${toCpdError(e).message}`);
    return r.abort.signal.aborted ? none : { nudge: false, reached: false };
  }
}
