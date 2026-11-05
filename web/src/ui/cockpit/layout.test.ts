import { expect, it, vi } from "vitest";
import { resetLayout } from "./layout.ts";

it("resetLayout sets tree 20 center 58 inbox 22 and plan 40", () => {
  const panel = () => ({ resize: vi.fn() });
  const panels = { tree: panel(), center: panel(), inbox: panel(), plan: panel() };
  resetLayout(panels);
  expect(panels.tree.resize).toHaveBeenCalledExactlyOnceWith("20%");
  expect(panels.center.resize).toHaveBeenCalledExactlyOnceWith("58%");
  expect(panels.inbox.resize).toHaveBeenCalledExactlyOnceWith("22%");
  expect(panels.plan.resize).toHaveBeenCalledExactlyOnceWith("40%");
});
