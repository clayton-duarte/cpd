import { readFileSync } from "node:fs";
import { CpdError, usage, failure } from "./lib/errors.ts";
import { emit, errorResult, type Result } from "./lib/output.ts";
import * as doctor from "./commands/doctor.ts";
import * as tiers from "./commands/tiers.ts";
import * as leadStart from "./commands/lead-start.ts";
import * as leadAttach from "./commands/lead-attach.ts";
import * as workflowInit from "./commands/workflow-init.ts";
import * as workflowShow from "./commands/workflow-show.ts";
import * as workflowPhase from "./commands/workflow-phase.ts";
import * as jobAdd from "./commands/job-add.ts";
import * as jobEdit from "./commands/job-edit.ts";
import * as jobList from "./commands/job-list.ts";
import * as jobShow from "./commands/job-show.ts";
import * as jobCancel from "./commands/job-cancel.ts";
import * as jobRetry from "./commands/job-retry.ts";
import * as jobDispatch from "./commands/job-dispatch.ts";
import * as planRender from "./commands/plan-render.ts";
import * as planDecision from "./commands/plan-decision.ts";
import * as planLock from "./commands/plan-lock.ts";
import * as watch from "./commands/watch.ts";
import * as attemptRun from "./commands/attempt-run.ts";
import * as attemptSubmit from "./commands/attempt-submit.ts";
import * as attemptRead from "./commands/attempt-read.ts";
import * as attemptList from "./commands/attempt-list.ts";
import * as attemptShow from "./commands/attempt-show.ts";
import * as reviewDecide from "./commands/review-decide.ts";
import * as ask from "./commands/ask.ts";
import * as answer from "./commands/answer.ts";
import * as prOpen from "./commands/pr-open.ts";
import * as prPoll from "./commands/pr-poll.ts";
import * as prRebase from "./commands/pr-rebase.ts";

type Command = { run: (argv: string[]) => Promise<Result> };

/** Explicit dispatch table: key is "<group> <command>" or "<command>". */
const COMMANDS: Record<string, Command> = {
  doctor,
  tiers,
  "lead start": leadStart,
  "lead attach": leadAttach,
  "workflow init": workflowInit,
  "workflow show": workflowShow,
  "workflow phase": workflowPhase,
  "job add": jobAdd,
  "job edit": jobEdit,
  "job list": jobList,
  "job show": jobShow,
  "job cancel": jobCancel,
  "job retry": jobRetry,
  "job dispatch": jobDispatch,
  "plan render": planRender,
  "plan decision": planDecision,
  "plan lock": planLock,
  watch,
  "attempt run": attemptRun,
  "attempt submit": attemptSubmit,
  "attempt read": attemptRead,
  "attempt list": attemptList,
  "attempt show": attemptShow,
  "review decide": reviewDecide,
  ask,
  answer,
  "pr open": prOpen,
  "pr poll": prPoll,
  "pr rebase": prRebase,
};

const GROUPS = new Set(Object.keys(COMMANDS).filter((k) => k.includes(" ")).map((k) => k.split(" ")[0]));

function version(): string {
  const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  if (typeof pkg.version !== "string") throw failure("bad_package", "package.json has no version");
  return pkg.version;
}

async function dispatch(argv: string[]): Promise<Result> {
  const [first, second, ...rest] = argv;
  if (first === "--help") return { ok: true, commands: Object.keys(COMMANDS) };
  if (first === "--version") return { ok: true, version: version() };
  if (first === undefined) throw usage("usage", "no command given; try `cpd --help`");
  if (GROUPS.has(first)) {
    const key = `${first} ${second}`;
    const cmd = second === undefined ? undefined : COMMANDS[key];
    if (!cmd) throw usage("unknown_command", `unknown command: ${[first, second].filter(Boolean).join(" ")}`);
    return cmd.run(rest);
  }
  const cmd = COMMANDS[first];
  if (!cmd) throw usage("unknown_command", `unknown command: ${first}`);
  return cmd.run(argv.slice(1));
}

export async function main(argv: string[]): Promise<void> {
  try {
    emit(await dispatch(argv));
  } catch (e) {
    if (e instanceof CpdError) emit(errorResult(e));
    else emit(errorResult(failure("unexpected", e instanceof Error ? e.message : String(e))));
  }
}
