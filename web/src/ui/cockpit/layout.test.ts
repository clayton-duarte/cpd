import { expect, it, vi } from "vitest";
import { resetLayout } from "./layout.ts";

it("resetLayout sets tree 20 center 50 inbox 30 and plan 40", () => {
  const panel = () => ({ resize: vi.fn() });
  const panels = { tree: panel(), center: panel(), inbox: panel(), plan: panel() };
  resetLayout(panels);
  expect(panels.tree.resize).toHaveBeenCalledExactlyOnceWith("20%");
  expect(panels.center.resize).toHaveBeenCalledExactlyOnceWith("50%");
  expect(panels.inbox.resize).toHaveBeenCalledExactlyOnceWith("30%");
  expect(panels.plan.resize).toHaveBeenCalledExactlyOnceWith("40%");
});
