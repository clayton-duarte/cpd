// @vitest-environment jsdom
// Red tests for the shared Trace typography of the Reasoning block and the tool calls (air-d79):
// both render through web/src/ui/components/trace.tsx, so a tool row's line and a tool group's
// header line carry the same class set as the Reasoning block's line. Pins the shared structure,
// never the CSS values.
import { expect, it } from "vitest";
import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { ToolGroup } from "./ToolGroup.tsx";
import { ToolRow } from "./ToolRow.tsx";
import { Transcript } from "./Transcript.tsx";

const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };

// A non-error tool row: status "done" with no result, so it renders the plain row with no
// collapsible and no semantic (error/running) modifier on its line.
const doneTool = (id: string): ToolItem => ({
  kind: "tool",
  id: `tool:${id}`,
  callId: id,
  name: "read",
  args: { path: `/src/${id}.ts` },
  status: "done",
});

// One thinking block becomes the Reasoning block inside an expanded chain of thought.
const thought: TranscriptItem = {
  kind: "assistant",
  id: "a1",
  streaming: false,
  blocks: [{ type: "thinking", text: "pondering" }],
};

/** The class tokens of a rendered part, order-independent: its resolved typography. */
const classesOf = (container: HTMLElement, selector: string): string[] => {
  const el = container.querySelector(selector);
  expect(el, `element for ${selector}`).not.toBeNull();
  return Array.from((el as Element).classList).sort();
};

/** Renders the Reasoning block and returns its Trace line's class set. */
const reasoningLineClasses = (): string[] => {
  const m = mount(<Transcript {...base} thinking="expanded" items={[thought]} />);
  const classes = classesOf(m.container, '[data-kind="thinking"] [data-slot="trace-line"]');
  m.unmount();
  return classes;
};

it("the tool row line renders the same Trace typography as the reasoning block", () => {
  const reasoning = reasoningLineClasses();

  const m = mount(<ToolRow tool={doneTool("c1")} />);
  const row = classesOf(m.container, '[data-kind="tool"] [data-slot="trace-line"]');
  m.unmount();

  expect(row).toEqual(reasoning);
});

it("the tool group header renders the same Trace typography as the reasoning block", () => {
  const reasoning = reasoningLineClasses();

  const m = mount(<ToolGroup tools={[doneTool("c1"), doneTool("c2")]} mode="grouped" />);
  const header = classesOf(m.container, '[data-part="group-header"] [data-slot="trace-line"]');
  m.unmount();

  expect(header).toEqual(reasoning);
});
