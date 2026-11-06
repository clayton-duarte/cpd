import { useCallback, useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactElement } from "react";
import { Button } from "../components/button.tsx";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../components/collapsible.tsx";
import { flowEdges } from "./flow-edges.ts";
import type { FlowBox, FlowEdge } from "./flow-edges.ts";
import { FLOW_STEPS } from "./flow-steps.ts";

const PALETTE: Record<string, string> = {
  context: "border-sakura/50 bg-sakura/12 text-sakura",
  manager: "border-sakura-bright/60 bg-sakura-bright/15 text-sakura-bright",
  plan: "border-shiro/50 bg-shiro/12 text-shiro",
  lead: "border-shiro-bright/60 bg-shiro-bright/15 text-shiro-bright",
  dispatch: "border-matcha/50 bg-matcha/12 text-matcha",
  worker: "border-matcha-bright/60 bg-matcha-bright/15 text-matcha-bright",
};
const badge = "rounded-[4px] border px-1.5 py-px font-mono text-meta";

interface Measured { readonly width: number; readonly height: number; readonly edges: FlowEdge[] }

export function CpdFlowCanvas(): ReactElement {
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const [measured, setMeasured] = useState<Measured>({ width: 0, height: 0, edges: [] });
  const container = useRef<HTMLDivElement>(null);
  const cards = useRef(new Map<string, HTMLLIElement>());
  const markerId = `${useId()}-arrow`;

  const measure = useCallback(() => {
    const c = container.current;
    if (c === null) return;
    const boxes: FlowBox[] = [];
    let left = 0;
    let width = 0;
    for (const s of FLOW_STEPS) {
      const el = cards.current.get(s.id);
      if (el === undefined) return;
      boxes.push({ id: s.id, top: el.offsetTop, height: el.offsetHeight });
      left = el.offsetLeft;
      width = el.offsetWidth;
    }
    const loops = FLOW_STEPS.flatMap((s) => (s.loop === null ? [] : [{ from: s.id, to: s.loop.to }]));
    setMeasured({ width: c.offsetWidth, height: c.offsetHeight, edges: flowEdges(boxes, { left, width }, loops) });
  }, []);

  useLayoutEffect(() => {
    measure();
    const ro = new ResizeObserver(() => measure());
    if (container.current !== null) ro.observe(container.current);
    for (const el of cards.current.values()) ro.observe(el);
    return () => ro.disconnect();
  }, [measure, open]);

  const all = open.size === FLOW_STEPS.length;
  const toggle = (id: string, v: boolean) =>
    setOpen((prev) => { const n = new Set(prev); if (v) n.add(id); else n.delete(id); return n; });

  return (
    <div data-part="flow-canvas" className="flex flex-col gap-2">
      <div>
        <Button data-part="expand-all" variant="outline" size="sm"
          onClick={() => setOpen(all ? new Set() : new Set(FLOW_STEPS.map((s) => s.id)))}>
          {all ? "Collapse all" : "Expand all"}
        </Button>
      </div>
      <div ref={container} className="relative pr-28">
        <svg data-part="dag-edges" aria-hidden="true" className="pointer-events-none absolute left-0 top-0"
          width={measured.width} height={measured.height} viewBox={`0 0 ${measured.width} ${measured.height}`} fill="none">
          <defs>
            <marker id={markerId} markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-line" />
            </marker>
          </defs>
          {measured.edges.map((e) => e.kind === "forward" ? (
            <path key={e.id} data-edge={e.id} d={e.d} strokeWidth={1.5} className="stroke-line"
              markerEnd={`url(#${markerId})`} />
          ) : (
            <path key={e.id} data-edge={e.id} d={e.d} strokeWidth={1.5} className="stroke-sakura"
              strokeDasharray="4 3" />
          ))}
        </svg>
        <ul className="m-0 flex list-none flex-col gap-4 p-0">
          {FLOW_STEPS.map((s) => (
            <li key={s.id} data-step={s.id}
              ref={(el) => { if (el === null) cards.current.delete(s.id); else cards.current.set(s.id, el); }}
              className="rounded-[6px] border bg-card font-mono text-meta">
              <Collapsible open={open.has(s.id)} onOpenChange={(v) => toggle(s.id, v)}>
                <CollapsibleTrigger data-part="step-toggle" aria-label={`Toggle step ${s.id}`}
                  className="flex w-full flex-wrap items-center gap-2 p-2 text-left">
                  <span className="text-muted-foreground">{s.id}</span>
                  <span>{s.name}</span>
                  <span data-part="phase" className={`${badge} ${PALETTE[s.phase]}`}>{s.phase}</span>
                  <span data-part="agent" className={`${badge} ${PALETTE[s.agent]}`}>{s.agent}</span>
                  {s.artifact === null ? null : (
                    <span data-part="artifact" className={`${badge} text-muted-foreground`}>{s.artifact}</span>
                  )}
                  {s.loop === null ? null : (
                    <span data-part="loop" className={`${badge} border-sakura/50 text-sakura`}>{`${s.loop.to} · ${s.loop.label}`}</span>
                  )}
                </CollapsibleTrigger>
                <CollapsibleContent className="px-2 pb-2 text-muted-foreground">
                  <p data-part="step-description" className="m-0">{s.description}</p>
                  {s.loop === null ? null : (
                    <p data-part="loop-condition" className="m-0 mt-1">Loops while: {s.loop.condition}</p>
                  )}
                </CollapsibleContent>
              </Collapsible>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
