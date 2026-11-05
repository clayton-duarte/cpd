// @vitest-environment jsdom
import { act } from "react";
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { ChatHeader } from "./ChatHeader.tsx";

it("shows only the title", () => {
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix the bug" role="lead" runner="pi" />);
  expect(m.container.querySelector("header h1")?.textContent).toBe("Fix the bug");
  expect(m.container.querySelector("header")?.textContent).toBe("Fix the bug");
  m.unmount();
  const m2 = mount(<ChatHeader sessionKey="lead-1" title={null} />);
  expect(m2.container.querySelector("header h1")?.textContent).toBe("lead-1");
  m2.unmount();
});

it("sets document.title from the session key and title", () => {
  document.title = "before";
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix the bug" />);
  expect(document.title).toBe("lead-1 · Fix the bug");
  act(() => m.unmount());
  const m2 = mount(<ChatHeader sessionKey="lead-2" title={null} />);
  expect(document.title).toBe("lead-2");
  m2.unmount();
});

it("restores the document title on unmount", () => {
  document.title = "original";
  const m = mount(<ChatHeader sessionKey="lead-1" title="T" />);
  expect(document.title).toBe("lead-1 · T");
  m.unmount();
  expect(document.title).toBe("original");
});
