export type FlowPhase = "context" | "plan" | "dispatch";
export type FlowAgent = "manager" | "lead" | "worker";
export interface FlowLoop {
  readonly to: string;
  readonly label: string;
  readonly condition: string;
}
export interface FlowStep {
  readonly id: string;
  readonly name: string;
  readonly phase: FlowPhase;
  readonly agent: FlowAgent;
  readonly artifact: string | null;
  readonly description: string;
  readonly loop: FlowLoop | null;
}

export const FLOW_STEPS: readonly FlowStep[] = [
  {
    id: "01",
    name: "Intake raw input",
    phase: "context",
    agent: "manager",
    artifact: "Plan (shaping)",
    description: "The User, or an external ticket, reaches the Manager as raw direction. The Manager creates the Plan in the `shaping` phase with no Lead yet; this is planned, not implemented (see the seams below).",
    loop: null,
  },
  {
    id: "02",
    name: "Explore",
    phase: "context",
    agent: "manager",
    artifact: "explore/<job>.md",
    description: "The Manager dispatches a `scout` Job (Tier H3, reads this machine) and a `research` Job (Tier S1 or higher, may reach outside). Each writes an Explore Report at `explore/<job>.md`, and the step loops while a question is open.",
    loop: { to: "02", label: "more explore", condition: "A question from an Explore Report is still open." },
  },
  {
    id: "03",
    name: "Grill the User",
    phase: "context",
    agent: "manager",
    artifact: "Context Records",
    description: "The Manager interviews the User with structured questions and records each settled choice as a Context Record with `cpd context decide` or `cpd context note`. The step loops until no decision is open.",
    loop: { to: "03", label: "ask again", condition: "A decision is still open with the User." },
  },
  {
    id: "04",
    name: "Write the brief",
    phase: "context",
    agent: "manager",
    artifact: "Plan Brief",
    description: "The Manager writes the Plan Brief (D3), its handoff to the Lead.",
    loop: null,
  },
  {
    id: "05",
    name: "Spawn the Lead",
    phase: "context",
    agent: "manager",
    artifact: null,
    description: "The Manager spawns the Plan's Lead with the brief, then goes dormant until Step 20.",
    loop: null,
  },
  {
    id: "06",
    name: "Ground",
    phase: "context",
    agent: "lead",
    artifact: null,
    description: "The Lead grounds itself on the brief and the Context Records, adds `scout` and `research` Jobs for the repo facts it still lacks, then runs `cpd plan phase plan`.",
    loop: null,
  },
  {
    id: "07",
    name: "Clarify",
    phase: "plan",
    agent: "lead",
    artifact: "Assumptions A1..An",
    description: "The Lead restates the Requirements and lists numbered assumptions A1..An for the User to accept.",
    loop: null,
  },
  {
    id: "08",
    name: "Scope",
    phase: "plan",
    agent: "lead",
    artifact: "Workflows",
    description: "The Lead cuts the work into Workflows, one repo each, with `cpd workflow add`. A new assumption sends the step back to Step 07.",
    loop: { to: "07", label: "new assumption", condition: "A new assumption comes up while scoping." },
  },
  {
    id: "09",
    name: "Split",
    phase: "plan",
    agent: "lead",
    artifact: "Job Specs",
    description: "The Lead writes a Job Spec per Job (goal, context, touches, acceptance, out_of_scope). Each Spec is the only input its Worker sees (D6). The Jobs of a Workflow then run in the order docs, red tests, implementation (Steps 14\u201316).",
    loop: null,
  },
  {
    id: "10",
    name: "Outline",
    phase: "plan",
    agent: "lead",
    artifact: "Plan Outline",
    description: "The Lead writes the Plan Outline with `cpd plan outline`: goal, decisions, assumptions, risks, done_when and out_of_scope.",
    loop: null,
  },
  {
    id: "11",
    name: "Review",
    phase: "plan",
    agent: "lead",
    artifact: null,
    description: "The User reviews the Plan and answers Approve or Change.",
    loop: null,
  },
  {
    id: "12",
    name: "Lock",
    phase: "plan",
    agent: "lead",
    artifact: "Lead Branch <plan>/<wf>",
    description: "`cpd plan lock` queues the Jobs, creates each started Workflow's Lead Branch `<plan>/<wf>` and its worktree, and starts the engine.",
    loop: null,
  },
  {
    id: "13",
    name: "Queue later batches",
    phase: "plan",
    agent: "lead",
    artifact: null,
    description: "The Lead registers later batches, either queued with `--needs` or as drafts. A draft that needs another review goes back to Step 11.",
    loop: { to: "11", label: "re-review", condition: "A later batch needs another User review." },
  },
  {
    id: "14",
    name: "Document",
    phase: "dispatch",
    agent: "worker",
    artifact: "Docs Commit",
    description: "A Worker commits the docs amendment. The output is the Docs Commit.",
    loop: null,
  },
  {
    id: "15",
    name: "Red tests",
    phase: "dispatch",
    agent: "worker",
    artifact: "Red Test Commit",
    description: "A different Worker commits red tests that fail for the right reason. The output is the Red Test Commit.",
    loop: null,
  },
  {
    id: "16",
    name: "Implement",
    phase: "dispatch",
    agent: "worker",
    artifact: "Green Impl Commit",
    description: "A Worker commits the implementation that makes the red tests pass. The output is the Green Impl Commit. A failed Attempt retries, which loops back to this step.",
    loop: { to: "16", label: "retry", condition: "An Attempt fails and is retried." },
  },
  {
    id: "17",
    name: "Merge into Lead Branch",
    phase: "dispatch",
    agent: "lead",
    artifact: "Lead Branch Commit",
    description: "The engine merges each submitted Job into the Lead Branch as one commit, and the Worker terminates. The Lead acts as air-traffic controller here.",
    loop: null,
  },
  {
    id: "18",
    name: "Open pull request",
    phase: "dispatch",
    agent: "lead",
    artifact: "Pull Request",
    description: "The Workflow's open Job merges the base if it conflicts, runs the Gate, pushes and opens the Pull Request. A Gate or CI failure appends a fix Job, which loops back to Step 16.",
    loop: { to: "16", label: "fix gate or CI", condition: "The Gate or CI fails on the pull request." },
  },
  {
    id: "19",
    name: "Review and reply",
    phase: "dispatch",
    agent: "lead",
    artifact: "PR Review / Reply",
    description: "Reviewers comment on the Pull Request. The Lead appends one fix Job per review round and replies to each comment, which loops back to Step 16.",
    loop: { to: "16", label: "fix review", condition: "Reviewers leave comments on the pull request." },
  },
  {
    id: "20",
    name: "Merge and close",
    phase: "dispatch",
    agent: "manager",
    artifact: "Merged PR (done)",
    description: "The Pull Request merges, the Plan becomes `done` and the Manager wakes. Follow-up work starts a new Outline, which loops back to Step 10.",
    loop: { to: "10", label: "follow-up work", condition: "Follow-up work starts a new Outline." },
  },
];

export function forwardEdgeIds(): readonly string[] {
  return FLOW_STEPS.slice(1).map((s, i) => `${FLOW_STEPS[i]?.id}>${s.id}`);
}

export function loopEdgeIds(): readonly string[] {
  return FLOW_STEPS.flatMap((s) => (s.loop === null ? [] : [`loop:${s.id}>${s.loop.to}`]));
}
