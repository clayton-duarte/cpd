// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { usePersistedBoolean } from "./persisted.ts";

function fake(initial: Record<string, string> = {}): Storage {
  const m = new Map(Object.entries(initial));
  return {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k: string) => m.get(k) ?? null,
    key: (i: number) => Array.from(m.keys())[i] ?? null,
    removeItem: (k: string) => void m.delete(k),
    setItem: (k: string, v: string) => void m.set(k, String(v)),
  };
}

afterEach(() => vi.unstubAllGlobals());

let setter: ((v: boolean) => void) | null = null;
function Probe(props: { k: string; d: boolean }) {
  const [v, set] = usePersistedBoolean(props.k, props.d);
  setter = set;
  return <span data-v={String(v)} />;
}
const val = (c: HTMLElement) => c.querySelector("span")?.getAttribute("data-v");

it("usePersistedBoolean reads the stored value at mount and writes on change", () => {
  const store = fake({ "cpd-ui:collapse:a": "0", "cpd-ui:collapse:b": "1" });
  vi.stubGlobal("localStorage", store);
  const a = mount(<Probe k="a" d={true} />);
  expect(val(a.container)).toBe("false");
  const b = mount(<Probe k="b" d={false} />);
  expect(val(b.container)).toBe("true");
  b.unmount();
  const c = mount(<Probe k="c" d={true} />);
  act(() => setter?.(false));
  expect(val(c.container)).toBe("false");
  expect(store.getItem("cpd-ui:collapse:c")).toBe("0");
  act(() => setter?.(true));
  expect(val(c.container)).toBe("true");
  expect(store.getItem("cpd-ui:collapse:c")).toBe("1");
  a.unmount();
  c.unmount();
});

it("usePersistedBoolean uses the default when nothing is stored", () => {
  const store = fake();
  vi.stubGlobal("localStorage", store);
  const open = mount(<Probe k="x" d={true} />);
  expect(val(open.container)).toBe("true");
  open.unmount();
  const closed = mount(<Probe k="y" d={false} />);
  expect(val(closed.container)).toBe("false");
  closed.unmount();
  expect(store.length).toBe(0);
  const dflt = mount(<Probe k="z" d={false} />);
  act(() => setter?.(true));
  expect(store.getItem("cpd-ui:collapse:z")).toBe("1");
  dflt.unmount();
});
