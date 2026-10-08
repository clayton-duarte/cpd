/**
 * Pure derivation: data -> the per-level list each upper canvas level draws.
 * Counts only (no status roll-up, per card spec) -- no React, no DOM.
 */
import type { CpdData, Session, Workflow } from './types';

export interface LeadCardData {
  id: string;
  name: string;
  leadTier: Session['leadTier'];
  planCount: number;
}

export interface PlanCardData {
  id: string;
  title: string;
  ticket?: Workflow['ticket'];
  branch?: Workflow['branch'];
  pr?: Workflow['pr'];
  jobCount: number;
}

export function leadsLevel(data: CpdData): LeadCardData[] {
  return data.sessions.map((session) => ({
    id: session.id,
    name: session.name,
    leadTier: session.leadTier,
    planCount: session.workflowIds.length,
  }));
}

export function plansLevel(data: CpdData, leadId: string): PlanCardData[] {
  const session = data.sessions.find((s) => s.id === leadId);
  if (!session) return [];
  return session.workflowIds
    .map((id) => data.workflows.find((w) => w.id === id))
    .filter((w): w is Workflow => w !== undefined)
    .map((workflow) => ({
      id: workflow.id,
      title: workflow.title,
      ticket: workflow.ticket,
      branch: workflow.branch,
      pr: workflow.pr,
      jobCount: workflow.jobIds.length,
    }));
}

/** Data scoped to a single workflow, for the unchanged Jobs-level Canvas. */
export function dataForPlan(data: CpdData, planId: string): CpdData {
  const workflow = data.workflows.find((w) => w.id === planId);
  if (!workflow) return { ...data, sessions: [], workflows: [], jobs: [] };
  const session = data.sessions.find((s) => s.id === workflow.sessionId);
  const jobs = data.jobs.filter((j) => j.workflowId === planId);
  return {
    projects: data.projects,
    sessions: session ? [session] : [],
    workflows: [workflow],
    jobs,
  };
}
