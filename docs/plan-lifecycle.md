# Plan lifecycle

## Overview

A Plan is one end-to-end unit of work. There is no separate Issue entity. An external Jira or GitHub issue is raw input direction: it is researched and shaped into a Plan, and it is never treated as truth.

Every Plan moves through three stages:

- Context: the Manager gathers raw direction, explores, and interviews the User (Steps 01–05), then hands off to the Lead (Step 06).
- Plan: the Lead clarifies, scopes, splits, outlines and locks the work with the User's approval (Steps 07–13).
- Dispatch: Workers build the work into Lead Branches, and the Lead opens, reviews and merges the pull requests (Steps 14–20).

Three kinds of agent act on a Plan:

- Manager: talks to the User and the outside world, and owns the Plan before and after the Lead works on it.
- Lead: one per Plan, plans the work and then steers the Workers.
- Worker: one per Job, does one unit of work and terminates.

## The 20 steps

| # | Step | Stage | Agent | Output artifact | Loop |
|---|------|-------|-------|-----------------|------|
| 01 | Intake raw input | context | manager | Plan (shaping) | — |
| 02 | Explore | context | manager | explore/<job>.md | 02 · more explore |
| 03 | Grill the User | context | manager | Context Records | 03 · ask again |
| 04 | Write the brief | context | manager | Plan Brief | — |
| 05 | Spawn the Lead | context | manager | — | — |
| 06 | Ground | context | lead | — | — |
| 07 | Clarify | plan | lead | Assumptions A1..An | — |
| 08 | Scope | plan | lead | Workflows | 07 · new assumption |
| 09 | Split | plan | lead | Job Specs | — |
| 10 | Outline | plan | lead | Plan Outline | — |
| 11 | Review | plan | lead | — | — |
| 12 | Lock | plan | lead | Lead Branch <plan>/<wf> | — |
| 13 | Queue later batches | plan | lead | — | 11 · re-review |
| 14 | Document | dispatch | worker | Docs Commit | — |
| 15 | Red tests | dispatch | worker | Red Test Commit | — |
| 16 | Implement | dispatch | worker | Green Impl Commit | 16 · retry |
| 17 | Merge into Lead Branch | dispatch | lead | Lead Branch Commit | — |
| 18 | Open pull request | dispatch | lead | Pull Request | 16 · fix gate or CI |
| 19 | Review and reply | dispatch | lead | PR Review / Reply | 16 · fix review |
| 20 | Merge and close | dispatch | manager | Merged PR (done) | 10 · follow-up work |

### 01 Intake raw input

The User, or an external ticket, reaches the Manager as raw direction. The Manager creates the Plan in the `shaping` phase with no Lead yet; the phase `shaping` and the nullable `plans.lead_session` are implemented (seam 1 below), the Manager creates it and runs its explore Jobs (seam 2 below), the rest of the flow is planned (seams 3 and 4).

### 02 Explore

The Manager dispatches a `scout` Job (Tier H3, reads this machine) and a `research` Job (Tier S1 or higher, may reach outside). Each writes an Explore Report at `explore/<job>.md`, and the step loops while a question is open.

### 03 Grill the User

The Manager interviews the User with structured questions and records each settled choice as a Context Record with `cpd context decide` or `cpd context note`. The step loops until no decision is open.

### 04 Write the brief

The Manager writes the Plan Brief (D3), its handoff to the Lead.

### 05 Spawn the Lead

The Manager spawns the Plan's Lead with the brief, then goes dormant until Step 20.

### 06 Ground

The Lead grounds itself on the brief and the Context Records, adds `scout` and `research` Jobs for the repo facts it still lacks, then runs `cpd plan phase plan`.

### 07 Clarify

The Lead restates the Requirements and lists numbered assumptions A1..An for the User to accept.

### 08 Scope

The Lead cuts the work into Workflows, one repo each, with `cpd workflow add`. A new assumption sends the step back to Step 07.

### 09 Split

The Lead writes a Job Spec per Job (goal, context, touches, acceptance, out_of_scope). Each Spec is the only input its Worker sees (D6). The Jobs of a Workflow then run in the order docs, red tests, implementation (Steps 14–16).

### 10 Outline

The Lead writes the Plan Outline with `cpd plan outline`: goal, decisions, assumptions, risks, done_when and out_of_scope.

### 11 Review

The User reviews the Plan and answers Approve or Change.

### 12 Lock

`cpd plan lock` queues the Jobs, creates each started Workflow's Lead Branch `<plan>/<wf>` and its worktree, and starts the engine.

### 13 Queue later batches

The Lead registers later batches, either queued with `--needs` or as drafts. A draft that needs another review goes back to Step 11.

### 14 Document

A Worker commits the docs amendment. The output is the Docs Commit.

### 15 Red tests

A different Worker commits red tests that fail for the right reason. The output is the Red Test Commit.

### 16 Implement

A Worker commits the implementation that makes the red tests pass. The output is the Green Impl Commit. A failed Attempt retries, which loops back to this step.

### 17 Merge into Lead Branch

The engine merges each submitted Job into the Lead Branch as one commit, and the Worker terminates. The Lead acts as air-traffic controller here.

### 18 Open pull request

The Workflow's open Job merges the base if it conflicts, runs the Gate, pushes and opens the Pull Request. A Gate or CI failure appends a fix Job, which loops back to Step 16.

### 19 Review and reply

Reviewers comment on the Pull Request. The Lead appends one fix Job per review round and replies to each comment, which loops back to Step 16.

### 20 Merge and close

The Pull Request merges, the Plan becomes `done` and the Manager wakes. Follow-up work starts a new Outline, which loops back to Step 10.

## Documents

### D1 Plan

The Plan record. Its phase moves through `shaping`, `grounding`, `plan`, `dispatch`, `deploy` and `done`. `lead_session` is null only while the phase is `shaping`.

### D2 Context Records

The decisions and notes made during the Plan. They are append-only and shared by every agent on the Plan.

### D3 Plan Brief

The Manager's handoff to the Lead. The Manager writes it in Step 04, and the Lead grounds on it in Step 06.

### D4 Explore Report

`explore/<job>.md`, written by a `scout` or `research` Worker. Only its summary, at most 1500 characters, reaches the asker.

### D5 Workflows and Plan Outline

The Workflows, and the Plan Outline with a `done_when` per Workflow. The `done_when` criteria become the pull request's acceptance criteria.

### D6 Job Specs

One Spec per Job. A Worker sees only its own Spec.

### D7 Lead Branch Commits and Pull Request

One commit per merged Job on `<plan>/<wf>`, then one Pull Request per Workflow.

## Agent lifecycles

### Manager

The Manager is U-shaped per Plan. It is active in Steps 01–05 and again in Step 20, and it is dormant in Steps 06–19.

### Lead

The Lead has two hats. As Architect (Steps 06–13) it grounds, clarifies, scopes, splits, outlines, reviews, locks and queues the work. As Air-Traffic Controller (Steps 14–20) it reacts to events, answers escalations, appends fix Jobs and replies to reviews.

### Worker

The Worker is a stateless function: 1 Worker = 1 Job = 1 Commit (or 1 Explore Report). It terminates immediately after its merge into `<plan>/<wf>`.

## Pre-Lead shaping and Manager explore

Steps 01–05 run before the Plan has a Lead. Four seams would let that work. Seams 1 and 2 are implemented: main now has the phase `shaping` and a nullable `plans.lead_session` (Plan `ctx-explore`, Workflow `schema`), and a Manager creates a Lead-less `shaping` Plan with `cpd plan new --shaping` and runs its explore Jobs (Plan `ctx-explore`, Workflow `cmds`). Seams 3 and 4 are planned.

1. `plans.lead_session` is nullable while the phase is `shaping`, and the phase `shaping` exists. Status: implemented (Plan `ctx-explore`, Workflow `schema`).
2. Scout and research Jobs are allowed on a Plan that has no Lead. Status: implemented (Plan `ctx-explore`, Workflow `cmds`).
3. The `job-merged`, `job-failed` and `escalation` events route to the Context's Manager session when `lead_session IS NULL`. Status: planned, not implemented.
4. The Lead handoff moves the Plan from `shaping` to `grounding` and sets `lead_session`. Status: planned, not implemented.
