// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { mount } from "../testing/render.tsx";
import { EngineBanner } from "./EngineBanner.tsx";

let view: { unmount(): void } | null = null;
afterEach(() => { view?.unmount(); view = null; });

it("renders nothing without a banner", () => {
  const v = (view = mount(<EngineBanner banner={null} />));
  expect(v.container.querySelector('[data-part="engine-banner"]')).toBeNull();
});
it("renders the kind and since", () => {
  const v = (view = mount(<EngineBanner banner={{ kind: "engine-down", since: "2026-01-01T00:00:00Z" }} />));
  const el = v.container.querySelector('[data-part="engine-banner"]');
  expect(el?.getAttribute("data-kind")).toBe("engine-down");
  expect(el?.textContent).toBe("Engine is down since 2026-01-01T00:00:00Z");
});
it("omits since when unknown", () => {
  const v = (view = mount(<EngineBanner banner={{ kind: "reconnecting", since: null }} />));
  expect(v.container.textContent).toBe("Reconnecting to the server");
});

it("renders the install restart notice in the same banner", () => {
  const v = (view = mount(<EngineBanner banner={{ kind: "engine-restarting", since: "2026-01-15T12:00:00Z" }} />));
  expect(v.container.querySelectorAll('[data-part="engine-banner"]')).toHaveLength(1);
  expect(v.container.textContent).toBe("Engine is restarting for an install since 2026-01-15T12:00:00Z");
});
