// Bootstrap (plan.md "self-hosting point"): lock the v0 plan with ledger-core's lockPlan and record the existing
// Lead Branch worktree. `cpd plan lock` arrives with Job `watch`; this does its first two steps only.
import { lockPlan } from "/Users/claytonduarte/projects/cpd.cpd-v0/src/ledger/plan.ts";
import { readWorkflow, writeWorkflow } from "/Users/claytonduarte/projects/cpd.cpd-v0/src/ledger/workflow.ts";
import { createLeadBranch } from "/Users/claytonduarte/projects/cpd.cpd-v0/src/git/lead-branch.ts";
const wf = "v0";
const r = await lockPlan(wf);
const w = readWorkflow(wf);
const { path } = await createLeadBranch(w.repo, w.leadBranch, w.base);
await writeWorkflow(wf, { ...w, leadWorktree: path });
console.log(JSON.stringify({ ok: true, ...r, leadWorktree: path }));
