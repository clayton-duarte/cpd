// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { Shell } from "./Shell.tsx";

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
