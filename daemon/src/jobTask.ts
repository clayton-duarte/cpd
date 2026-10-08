import { defineTask, type ConversationId, type TaskId, type Tx } from "@earendil-works/pi-durable";
import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { PlanDoc, setStatus, type Job, type JobStatus } from "./plans.ts";

/** Bound the tail of a job's output kept durably once it settles: 200 lines or 16 KB, whichever
 * is smaller. `onOutput` is raw/unbounded/unthrottled (D114) -- buffer in memory, never commit
 * per chunk. */
const MAX_TAIL_LINES = 200;
const MAX_TAIL_BYTES = 16 * 1024;

export function boundTail(chunks: readonly string[]): string {
  const joined = chunks.join("");
  const lines = joined.split("\n");
  const tailLines = lines.slice(-MAX_TAIL_LINES).join("\n");
  const bytes = Buffer.byteLength(tailLines, "utf8");
  if (bytes <= MAX_TAIL_BYTES) return tailLines;
  // Trim from the front until within the byte budget; slicing a JS string by code unit is safe
  // here since we only need an approximate byte budget, not an exact UTF-8 boundary guarantee.
  let out = tailLines;
  while (Buffer.byteLength(out, "utf8") > MAX_TAIL_BYTES && out.length > 0) {
    out = out.slice(Math.ceil(out.length * 0.1));
  }
  return out;
}

export type JobTaskInput = {
  readonly jobId: string;
  readonly conversationId: ConversationId;
  readonly command?: string;
  /** Durable task ids of this job's dependencies, already resolved from job ids by the caller
   * (see `runPlanJob` in engine.ts). Never job ids -- the two id spaces are kept distinct in the
   * types so they cannot be silently confused again. */
  readonly dependsOn: readonly TaskId[];
};

export type JobTaskCheckpoint =
  | {
      readonly phase: "start";
      readonly jobId: string;
      readonly conversationId: ConversationId;
      readonly command?: string;
      readonly dependsOn: readonly TaskId[];
    }
  | {
      readonly phase: "run";
      readonly jobId: string;
      readonly conversationId: ConversationId;
      readonly command?: string;
    };

export type JobTaskResult = {
  readonly exitCode: number;
  readonly output: string;
};

/** Live output chunks buffered per task invocation, keyed by taskId; never persisted per chunk. */
const liveOutput = new Map<string, string[]>();

export type JobOutputListener = (taskId: string, jobId: string, conversationId: number, chunk: string) => void;
let outputListener: JobOutputListener | undefined;

/** Register the sink that streams live job output over SSE (`{type:"jobOutput"}`). Only one
 * listener at a time; the daemon's `index.ts` installs it once at boot. */
export function onJobOutput(listener: JobOutputListener | undefined): void {
  outputListener = listener;
}

/** Mirror a task's phase/status onto the job's `status` field in the conversation's plan
 * document, in the same commit as the task state change wherever the call site allows it. */
function mirrorStatus(tx: Tx, conversationId: ConversationId, jobId: string, status: JobStatus): Promise<unknown> {
  return tx.doc(PlanDoc, conversationId).then((draft) => {
    if (!draft.jobs.some((job) => job.id === jobId)) return;
    draft.jobs = setStatus(draft.jobs, jobId, status);
  });
}

/** Fired once a job's status change has actually committed to the plan document, regardless of
 * whether that change originated from an HTTP route or purely from the engine advancing a task
 * on its own (queued -> running -> done|failed, with no HTTP request in the loop at all). The
 * daemon's `index.ts` installs the sink that turns this into a live SSE `plan`/`attention` push
 * (J6) -- only one listener at a time, same convention as `onJobOutput`. */
export type JobStatusListener = (conversationId: number, jobId: string, status: JobStatus) => void;
const statusListeners = new Set<JobStatusListener>();

/** Register a listener; returns an unsubscribe function. Multiple listeners may be registered
 * at once (same convention as `onJobOutput`'s sibling pattern elsewhere, but here with a Set so
 * a second subscriber never silently displaces the first). */
export function onJobStatusChange(listener: JobStatusListener): () => void {
  statusListeners.add(listener);
  return () => {
    statusListeners.delete(listener);
  };
}

/** Call after a commit that used `mirrorStatus` has landed, so listeners only see settled
 * status changes (never a status that was committed then rolled back). A throwing listener is
 * caught and does not prevent the others from running or crash the daemon. */
function notifyStatus(conversationId: ConversationId, jobId: string, status: JobStatus): void {
  for (const listener of statusListeners) {
    try {
      listener(Number(conversationId), jobId, status);
    } catch (err) {
      console.error("onJobStatusChange listener threw", err);
    }
  }
}

export const JobTask = defineTask<JobTaskInput, JobTaskCheckpoint, JobTaskResult, object>({
  name: "cpd.job",
  version: 1,
  initial: (input) => ({
    phase: "start",
    jobId: input.jobId,
    conversationId: input.conversationId,
    command: input.command,
    dependsOn: input.dependsOn,
  }),
  phases: {
    start: async (task, rt, context) => {
      const { jobId, conversationId, command, dependsOn } = task.state.checkpoint;

      if (dependsOn.length === 0) {
        if (!command) {
          await rt.commit(async (tx) => {
            await mirrorStatus(tx, conversationId, jobId, "done");
            return { status: "terminal", outcome: { status: "completed", result: { exitCode: 0, output: "" } } };
          }, context);
          notifyStatus(conversationId, jobId, "done");
          return;
        }
        await rt.commit(async (tx) => {
          await mirrorStatus(tx, conversationId, jobId, "running");
          return {
            status: "running",
            checkpoint: { phase: "run", jobId, conversationId, command },
          };
        }, context);
        notifyStatus(conversationId, jobId, "running");
        return;
      }

      const depIds = dependsOn;
      await rt.commit(async (tx) => {
        await mirrorStatus(tx, conversationId, jobId, "queued");
        return {
          status: "waiting",
          checkpoint: { phase: "run", jobId, conversationId, command },
          on: depIds,
          policy: "allSettled",
        };
      }, context);
      notifyStatus(conversationId, jobId, "queued");
    },
    run: async (task, rt, context) => {
      const { jobId, conversationId, command } = task.state.checkpoint;

      if (!command) {
        await rt.commit(async (tx) => {
          await mirrorStatus(tx, conversationId, jobId, "done");
          return { status: "terminal", outcome: { status: "completed", result: { exitCode: 0, output: "" } } };
        }, context);
        notifyStatus(conversationId, jobId, "done");
        return;
      }

      const env = await rt.env(context);
      if (!env) {
        await rt.commit(async (tx) => {
          await mirrorStatus(tx, conversationId, jobId, "failed");
          return {
            status: "terminal",
            outcome: { status: "failed", error: { message: "No execution environment configured for this harness" } },
          };
        }, context);
        notifyStatus(conversationId, jobId, "failed");
        return;
      }

      await rt.commit(async (tx) => {
        await mirrorStatus(tx, conversationId, jobId, "running");
        return undefined;
      }, context);
      notifyStatus(conversationId, jobId, "running");

      const taskKey = String(rt.taskId);
      const chunks: string[] = [];
      liveOutput.set(taskKey, chunks);

      const result = await env.exec(
        command,
        {
          timeout: 10 * 60 * 1000,
          onOutput: (text) => {
            chunks.push(text);
            outputListener?.(taskKey, jobId, Number(conversationId), text);
          },
        },
        context,
      );
      liveOutput.delete(taskKey);

      const output = boundTail(chunks);

      if (!result.ok) {
        await rt.commit(async (tx) => {
          await mirrorStatus(tx, conversationId, jobId, "failed");
          return {
            status: "terminal",
            outcome: { status: "failed", error: { message: result.error.message } },
          };
        }, context);
        notifyStatus(conversationId, jobId, "failed");
        return;
      }

      const { exitCode, spillPath } = result.value;
      const status: JobStatus = exitCode === 0 ? "done" : "failed";
      const value: JobTaskResult = { exitCode, output };
      void spillPath;

      await rt.commit(async (tx) => {
        await mirrorStatus(tx, conversationId, jobId, status);
        if (exitCode === 0) {
          return { status: "terminal", outcome: { status: "completed", result: value } };
        }
        return {
          status: "terminal",
          outcome: { status: "failed", error: { message: `Command exited ${exitCode}` }, result: value },
        };
      }, context);
      notifyStatus(conversationId, jobId, status);
    },
  },
  abort: async (task, rt, context) => {
    const { jobId, conversationId } = task.state.checkpoint;
    await rt.commit(async (tx) => {
      await mirrorStatus(tx, conversationId, jobId, "failed");
      return {
        status: "terminal",
        outcome: { status: "failed", error: { message: "aborted" } },
      };
    }, context);
    notifyStatus(conversationId, jobId, "failed");
  },
});

export { ctx as jobTaskContext };
