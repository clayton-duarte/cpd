import type { ReactElement } from "react";
import { CpdFlowCanvas } from "../cockpit/FlowCanvas.tsx";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "../components/dialog.tsx";
import { ScrollArea } from "../components/scroll-area.tsx";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../components/tabs.tsx";

interface Artifact { readonly id: string; readonly title: string; readonly text: string }
interface Lifecycle { readonly id: "manager" | "lead" | "worker"; readonly title: string; readonly text: string }
interface Seam { readonly n: number; readonly text: string }

const PLANNED = "Status: planned, not implemented.";

const ARTIFACTS: readonly Artifact[] = [
  { id: "D1", title: "Plan", text: "The Plan record. Its phase moves from shaping → done through `grounding`, `plan`, `dispatch` and `deploy`. `lead_session` is null only while the phase is `shaping`." },
  { id: "D2", title: "Context Records", text: "The decisions and notes made during the Plan. They are append-only and shared by every agent on the Plan." },
  { id: "D3", title: "Plan Brief", text: "The Manager's handoff to the Lead. The Manager writes it in Step 04, and the Lead grounds on it in Step 06." },
  { id: "D4", title: "Explore Report", text: "`explore/<job>.md`, written by a `scout` or `research` Worker. Only its summary, at most 1500 characters, reaches the asker." },
  { id: "D5", title: "Workflows + Plan Outline", text: "The Workflows, and the Plan Outline with a `done_when` per Workflow. The `done_when` criteria become the pull request's acceptance criteria." },
  { id: "D6", title: "Job Specs", text: "One Spec per Job. A Worker sees only its own Spec." },
  { id: "D7", title: "Lead Branch Commits & Pull Request", text: "One commit per merged Job on `<plan>/<wf>`, then one Pull Request per Workflow." },
];

const LIFECYCLES: readonly Lifecycle[] = [
  { id: "manager", title: "Manager", text: "The Manager is U-shaped per Plan. It is active in Steps 01–05 and again in Step 20, and it is dormant in Steps 06–19." },
  { id: "lead", title: "Lead", text: "The Lead has two hats. As Architect (Steps 06–13) it grounds, clarifies, scopes, splits, outlines, reviews, locks and queues the work. As Air-Traffic Controller (Steps 14–20) it reacts to events, answers escalations, appends fix Jobs and replies to reviews." },
  { id: "worker", title: "Worker", text: "The Worker is a stateless function: 1 Worker = 1 Job = 1 Commit (or 1 Explore Report). It terminates immediately after its merge into `<plan>/<wf>`." },
];

const SEAMS: readonly Seam[] = [
  { n: 1, text: `\`plans.lead_session\` is nullable while \`phase = 'shaping'\`. ${PLANNED}` },
  { n: 2, text: `Scout and research Jobs are allowed on a Plan that has no Lead. ${PLANNED}` },
  { n: 3, text: `The \`job-merged\`, \`job-failed\` and \`escalation\` events route to the Context's Manager session when \`lead_session IS NULL\`. ${PLANNED}` },
  { n: 4, text: `The Lead handoff moves the Plan from \`shaping\` to \`grounding\` and sets \`lead_session\`. ${PLANNED}` },
];

export function DocsDialog(props: { open: boolean; onClose(): void }): ReactElement {
  return (
    <Dialog open={props.open} onOpenChange={(o) => { if (!o) props.onClose(); }}>
      <DialogContent data-part="docs-dialog" aria-describedby={undefined} className="overflow-hidden sm:max-w-5xl">
        <DialogHeader><DialogTitle>Documentation</DialogTitle></DialogHeader>
        <Tabs defaultValue="flowchart">
          <TabsList>
            <TabsTrigger value="flowchart" data-tab-trigger="flowchart">Issue Flowchart</TabsTrigger>
            <TabsTrigger value="artifacts" data-tab-trigger="artifacts">Documents &amp; Lifecycles</TabsTrigger>
          </TabsList>
          <ScrollArea className="h-[70vh]">
            <TabsContent value="flowchart" data-tab="flowchart"><CpdFlowCanvas /></TabsContent>
            <TabsContent value="artifacts" data-tab="artifacts" className="grid gap-4">
              <section className="grid gap-2">
                <h3 className="font-semibold text-fg-strong underline">Documents</h3>
                {ARTIFACTS.map((a) => (
                  <div key={a.id} data-artifact={a.id} className="text-meta">
                    <h4 className="font-semibold">{a.id} · {a.title}</h4>
                    <p className="text-muted-foreground">{a.text}</p>
                  </div>
                ))}
              </section>
              <section className="grid gap-2">
                <h3 className="font-semibold text-fg-strong underline">Agent lifecycles</h3>
                {LIFECYCLES.map((l) => (
                  <div key={l.id} data-lifecycle={l.id} className="text-meta">
                    <h4 className="font-semibold">{l.title}</h4>
                    <p className="text-muted-foreground">{l.text}</p>
                  </div>
                ))}
              </section>
              <section className="grid gap-2">
                <h3 className="font-semibold text-fg-strong underline">Pre-Lead shaping and Manager explore</h3>
                <p className="text-meta text-muted-foreground">Steps 01–05 run before the Plan has a Lead. Four seams would let that work.</p>
                {SEAMS.map((s) => (
                  <p key={s.n} data-seam={s.n} className="text-meta text-muted-foreground">{s.n}. {s.text}</p>
                ))}
              </section>
            </TabsContent>
          </ScrollArea>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
