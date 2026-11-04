// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { Shell } from "./Shell.tsx";

// The resizable wrappers are recorded: each part renders a div that exposes its props as data attributes.
vi.mock("../components/resizable.tsx", async () => {
  const { createElement } = await import("react");
  const part = (kind: string) => (props: Record<string, unknown>) => {
    const { children, withHandle, panelRef: _r, onResize: _o, ...rest } = props;
    const attrs: Record<string, unknown> = { "data-kind": kind, "data-with-handle": withHandle === true ? "true" : undefined };
    for (const [k, v] of Object.entries(rest)) attrs[k === "collapsible" ? "data-collapsible" : k] = typeof v === "boolean" ? String(v) : v;
    return createElement("div", attrs, children as never);
  };
  return {
    ResizablePanelGroup: part("group"),
    ResizablePanel: part("panel"),
    ResizableHandle: part("handle"),
    usePanelRef: () => ({ current: null }),
  };
});

// jsdom has no ResizeObserver, which react-resizable-panels (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });


it("lays out tree, plan view, chat slot and inbox", () => {
  const m = mount(
    <Shell tree={<i id="t">tree</i>} plan={<i id="p">plan</i>} chat={<i id="c">chat</i>} inbox={<i id="i">inbox</i>} />,
  );
  const shell = m.container.querySelector("div[data-shell]");
  expect(shell).not.toBeNull();
  expect(shell?.querySelector('aside[aria-label="Context tree column"] #t')).not.toBeNull();
  const center = shell?.querySelector('div[data-column="center"]');
  expect(center?.querySelector('section[aria-label="Plan pane"] #p')).not.toBeNull();
  expect(center?.querySelector('main[aria-label="Chat"] #c')).not.toBeNull();
  expect(shell?.querySelector('aside[aria-label="Inbox column"] #i')).not.toBeNull();
  m.unmount();
});

const slots = { tree: <i>t</i>, plan: <i>p</i>, chat: <i>c</i>, inbox: <i>i</i> };
const kids = (e: Element | null | undefined) => Array.from(e?.children ?? []);

it("the shell outer split is a horizontal resizable group with tree center and inbox panels", () => {
  const m = mount(<Shell {...slots} />);
  const group = m.container.querySelector('[data-shell] > [data-kind="group"]');
  expect(group?.getAttribute("id"), "group id").toBe("cpd-columns");
  expect(group?.getAttribute("data-part")).toBe("columns-group");
  expect(group?.getAttribute("orientation")).toBe("horizontal");
  expect(kids(group).map((e) => e.getAttribute("id"))).toEqual(["tree", "split-left", "center", "split-right", "inbox"]);
  expect(kids(group).map((e) => e.getAttribute("data-part"))).toEqual(["tree-panel", "split-left", "center-panel", "split-right", "inbox-panel"]);
  expect(group?.querySelector('#tree aside[aria-label="Context tree column"]')).not.toBeNull();
  expect(group?.querySelector('#center [data-column="center"][data-part="center-frame"]')).not.toBeNull();
  expect(group?.querySelector('#inbox aside[aria-label="Inbox column"]')).not.toBeNull();
  m.unmount();
});

it("the shell has a labelled handle between tree and center and between center and inbox", () => {
  const m = mount(<Shell {...slots} />);
  const left = m.container.querySelector("#split-left");
  const right = m.container.querySelector("#split-right");
  expect(left?.getAttribute("aria-label")).toBe("Resize context tree and center");
  expect(right?.getAttribute("aria-label")).toBe("Resize center and inbox");
  expect(left?.getAttribute("data-kind")).toBe("handle");
  expect(right?.getAttribute("data-kind")).toBe("handle");
  expect(left?.getAttribute("data-with-handle")).toBe("true");
  expect(right?.getAttribute("data-with-handle")).toBe("true");
  m.unmount();
});

it("the center column keeps the vertical plan and chat split with its labelled handle", () => {
  const m = mount(<Shell {...slots} />);
  const center = m.container.querySelector('#center [data-column="center"]');
  const group = center?.querySelector('[data-kind="group"]');
  expect(group?.getAttribute("id")).toBe("cpd-center");
  expect(group?.getAttribute("data-part")).toBe("center-group");
  expect(group?.getAttribute("orientation")).toBe("vertical");
  expect(kids(group).map((e) => e.getAttribute("id"))).toEqual(["plan", "split", "chat"]);
  const handle = group?.querySelector("#split");
  expect(handle?.getAttribute("aria-label")).toBe("Resize plan view and chat");
  expect(handle?.getAttribute("data-part")).toBe("split-handle");
  expect(handle?.getAttribute("data-with-handle")).toBe("true");
  expect(group?.querySelector('#plan section[aria-label="Plan pane"]')).not.toBeNull();
  expect(group?.querySelector('#chat main[aria-label="Chat"]')).not.toBeNull();
  m.unmount();
});

it("the shell panel sizes follow the decided defaults and limits", () => {
  const m = mount(<Shell {...slots} />);
  const sizes = (id: string) => {
    const e = m.container.querySelector(`#${id}`);
    return ["defaultSize", "minSize", "maxSize", "data-collapsible", "collapsedSize"].map((a) => e?.getAttribute(a));
  };
  expect(sizes("tree")).toEqual(["20%", "12%", "40%", "true", "0%"]);
  expect(sizes("center")).toEqual(["58%", "30%", null, null, null]);
  expect(sizes("inbox")).toEqual(["22%", "14%", "45%", "true", "0%"]);
  expect(sizes("plan")).toEqual(["40%", "20%", "80%", "true", "44px"]);
  expect(sizes("chat")).toEqual(["60%", "20%", null, "true", "0%"]);
  m.unmount();
});
