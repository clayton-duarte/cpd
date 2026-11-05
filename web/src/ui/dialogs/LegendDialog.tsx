import type { ReactElement } from "react";
import { cn } from "../../lib/utils.ts";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "../components/dialog.tsx";

/** A mark in a Dango tone, then its meaning (scout-legend §6, from the TUI legend). */
type Row = readonly [mark: string, tone: string, meaning: string];

const SECTIONS: readonly { title: string; rows: readonly Row[] }[] = [
  {
    title: "Meanings → Colour (One Source For Every View)",
    rows: [
      ["██", "text-muted-foreground", "not started"],
      ["██", "text-status-active", "in progress"],
      ["██", "text-status-landing", "waiting on review"],
      ["██", "text-status-done", "done"],
      ["██", "text-status-blocked", "failed"],
      ["██", "text-status-decision", "planning"],
      ["██", "text-muted-foreground", "grounding"],
      ["██", "text-fg-dim", "nothing / n/a / zero"],
      ["██", "text-status-active", "waiting on an agent"],
    ],
  },
  {
    title: "Jobs: Plan Pills And Context Counters",
    rows: [
      ["queued", "text-muted-foreground", "3"],
      ["running", "text-status-active", "3"],
      ["landing", "text-status-landing", "3"],
      ["merged", "text-status-done", "3"],
      ["failed", "text-status-blocked", "3"],
      ["draft", "text-status-decision", "3"],
      ["1⋅1⋅1⋅1", "text-fg-dim", "counter: planned⋅active⋅failed⋅done"],
      ["$0.120", "text-fg-dim", "cost: $0.012 ⋅ $1.23k ⋅ omitted when unknown"],
    ],
  },
  {
    title: "Session Dots",
    rows: [
      ["●", "text-fg-strong", "the Session needs you (colour below)"],
      ["◌", "text-status-blocked-bright", "the Lead is fixing a failed Job or answering a Worker's question"],
      ["◌", "text-status-landing-bright", "the Lead is working on a pull request (open, conflict, deploy, landing)"],
      ["◌", "text-status-active-bright", "waiting: an agent waits for another agent, not for you"],
      ["○", "text-fg-strong", "idle, nothing needed"],
      ["○", "text-fg-dim", "pi stopped"],
      ["●", "text-status-blocked-bright", "red: a dialog in dispatch or deploy, a stuck Job, an engine error, your own broken pull request that nobody is fixing"],
      ["●", "text-status-landing-bright", "yellow: ready to post, ready to merge, review requested"],
      ["●", "text-status-decision-bright", "magenta: blocked while grounding, planning or done, or the Plan's review is pending"],
      ["●", "text-status-active-bright", "cyan: a cpd notice that blocks nothing: curation"],
      ["●", "text-fg-strong", "white: idle, not seen since it last went idle"],
      ["◌", "text-status-decision-bright", "the Lead is planning (grounding, drafts, a follow-up)"],
      ["◌", "text-status-active-bright", "the Lead is working alongside running Jobs"],
      ["◌", "text-fg-strong", "a working Session with no Plan"],
    ],
  },
  {
    title: "Workflows (Dispatch)",
    rows: [
      ["#2 open", "text-fg-strong", ""],
      ["#2 merged", "text-status-done-bright", ""],
      ["#2 closed", "text-status-active-bright", "its Lead handles it"],
      ["#2 closed", "text-status-blocked-bright", "its Lead asks you"],
      ["#2 conflict", "text-status-blocked-bright", ""],
      ["#2 updating", "text-status-active-bright", "its update Job is fixing the conflict"],
      ["#2 updating", "text-status-active-bright", "its update Job is queued"],
    ],
  },
  {
    title: "Inbox",
    rows: [
      ["●", "text-status-blocked-bright", "a blocker only you can clear"],
      ["●", "text-status-landing-bright", "pull-request work"],
      ["●", "text-status-decision-bright", "a decision before the work runs"],
      ["●", "text-status-active", "blue, external support: a review asked of you"],
      ["●", "text-status-active-bright", "cyan, a cpd notice: curation, nothing blocks; also a change in the models a provider offers"],
      ["●", "text-fg-strong", "an unread reply"],
      ["●", "text-status-landing-bright", "a deploy waits for your approval"],
      ["●", "text-status-landing-bright", "a pull request of yours, coloured by its state"],
    ],
  },
  {
    title: "Pulse",
    rows: [
      ["▂▄▆", "text-foreground", "activity per 5 min, first colour wins"],
      ["▄", "text-status-blocked-bright", "an error, or a Job failed"],
      ["▄", "text-status-decision-bright", "a Plan or ask Event"],
      ["▄", "text-status-active-bright", "a pull request opened, an escalation, any other activity"],
      ["▄", "text-status-done-bright", "a Job or a Workflow merged"],
      ["▁", "text-fg-dim", "no activity"],
      ["quiet 41m", "text-fg-dim", "queued or running work with no activity for 30 min"],
    ],
  },
  {
    title: "PR Checks",
    rows: [
      ["passed", "text-status-done-bright", ""],
      ["failed", "text-status-blocked-bright", ""],
      ["pending", "text-status-active-bright", ""],
      ["none", "text-fg-dim", ""],
    ],
  },
];

export function LegendDialog(props: { open: boolean; onClose(): void }): ReactElement {
  return (
    <Dialog open={props.open} onOpenChange={(o) => { if (!o) props.onClose(); }}>
      <DialogContent data-part="legend-dialog" aria-describedby={undefined} className="sm:max-w-3xl">
        <DialogHeader><DialogTitle>Legend: What The Cockpit's Colours And Icons Mean</DialogTitle></DialogHeader>
        {SECTIONS.map((s) => (
          <section key={s.title} className="grid gap-1">
            <h3 className="font-semibold text-fg-strong underline">{s.title}</h3>
            {s.rows.map(([mark, tone, meaning], i) => (
              <div key={i} className="flex gap-3 text-meta">
                <span className={cn("w-24 shrink-0 font-mono", tone)}>{mark}</span>
                <span className="text-muted-foreground">{meaning}</span>
              </div>
            ))}
          </section>
        ))}
      </DialogContent>
    </Dialog>
  );
}
