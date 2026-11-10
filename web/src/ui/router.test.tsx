// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { createMemoryHistory, createHashHistory } from "@tanstack/react-router";
import { mount, act } from "../testing/render.tsx";
import { createAppRouter, AppRouterView } from "./router.tsx";

describe("router", () => {
  it("matches agent routes and renders corresponding view in middle column", async () => {
    // CONTRACT 22.2, 22.3, DESIGN-panes 5a
    // /a/$context -> agent-manager (ManagerView + chat)
    // /a/$context/$plan -> agent-lead (PlanView + chat)
    // /a/$context/$plan/$job -> agent-worker (PlanView of worker's plan + worker chat)
    const history = createMemoryHistory({ initialEntries: ["/a/cpd"] });
    const router = createAppRouter({ history });

    const { container, unmount } = mount(<AppRouterView router={router} />);
    await act(async () => {
      await router.load();
    });

    const middleColumn = container.querySelector('[data-part="middle-column"]');
    expect(middleColumn).not.toBeNull();
    expect(middleColumn?.getAttribute("data-route-state")).toBe("agent");

    // Navigate to lead route
    await act(async () => {
      await router.navigate({ to: "/a/$context/$plan", params: { context: "cpd", plan: "plan-x" } });
    });
    expect(container.querySelector('[data-part="middle-column"]')?.getAttribute("data-route-state")).toBe("agent");

    // Navigate to worker route
    await act(async () => {
      await router.navigate({
        to: "/a/$context/$plan/$job",
        params: { context: "cpd", plan: "plan-x", job: "web-tests" },
      });
    });
    expect(container.querySelector('[data-part="middle-column"]')?.getAttribute("data-route-state")).toBe("agent");
    unmount();
  });

  it("renders empty prompt on root route", async () => {
    // CONTRACT 22.3, DESIGN-panes 5b
    // Root / renders empty state prompt asking to select an agent
    // <section data-part="route-empty"><p>Select an agent in the tree or the Inbox.</p></section>
    const history = createMemoryHistory({ initialEntries: ["/"] });
    const router = createAppRouter({ history });

    const { container, unmount } = mount(<AppRouterView router={router} />);
    await act(async () => {
      await router.load();
    });

    const middleColumn = container.querySelector('[data-part="middle-column"]');
    expect(middleColumn?.getAttribute("data-route-state")).toBe("index");

    const emptySection = container.querySelector('section[data-part="route-empty"]');
    expect(emptySection).not.toBeNull();
    expect(emptySection?.textContent).toContain("Select an agent in the tree or the Inbox.");
    unmount();
  });

  it("renders not-found card with recovery button on unknown route without crashing", async () => {
    // CONTRACT 22.7, DESIGN-panes 5c
    // Middle column shows <section data-part="route-not-found"> with recovery button
    // Recovery button navigates to parent key (worker -> lead, lead -> manager, manager/unmatched -> /)
    const history = createMemoryHistory({ initialEntries: ["/a/cpd/plan-x/nonexistent-job"] });
    const router = createAppRouter({ history });

    const { container, unmount } = mount(<AppRouterView router={router} />);
    await act(async () => {
      await router.load();
    });

    const middleColumn = container.querySelector('[data-part="middle-column"]');
    expect(middleColumn?.getAttribute("data-route-state")).toBe("not-found");

    const notFoundCard = container.querySelector('section[data-part="route-not-found"]');
    expect(notFoundCard).not.toBeNull();
    expect(notFoundCard?.querySelector('[data-part="title"]')?.textContent).toBe("Agent not found");

    const recoverButton = notFoundCard?.querySelector('button[data-action="route-recover"]');
    expect(recoverButton).not.toBeNull();

    // Click recovery button to navigate to parent route
    await act(async () => {
      (recoverButton as HTMLButtonElement).click();
    });
    // Should navigate to lead: /a/cpd/plan-x
    expect(router.state.location.pathname).toBe("/a/cpd/plan-x");
    unmount();
  });

  it("preserves mounted context tree and inbox across route transitions", async () => {
    // CONTRACT 22.3, DESIGN 3j (A3)
    // ContextTree (left) and Inbox (right) are permanently mounted in root layout and never remounted on navigation.
    const history = createMemoryHistory({ initialEntries: ["/"] });
    const router = createAppRouter({ history });

    const { container, unmount } = mount(<AppRouterView router={router} />);
    await act(async () => {
      await router.load();
    });

    const treeNode = container.querySelector('[data-part="context-tree"]');
    const inboxNode = container.querySelector('[data-part="inbox"]');
    expect(treeNode).not.toBeNull();
    expect(inboxNode).not.toBeNull();

    // Navigate to manager route
    await act(async () => {
      await router.navigate({ to: "/a/$context", params: { context: "cpd" } });
    });
    expect(container.querySelector('[data-part="context-tree"]')).toBe(treeNode);
    expect(container.querySelector('[data-part="inbox"]')).toBe(inboxNode);

    // Navigate to lead route
    await act(async () => {
      await router.navigate({ to: "/a/$context/$plan", params: { context: "cpd", plan: "plan-x" } });
    });
    expect(container.querySelector('[data-part="context-tree"]')).toBe(treeNode);
    expect(container.querySelector('[data-part="inbox"]')).toBe(inboxNode);

    // Navigate to not-found route
    await act(async () => {
      await router.navigate({ to: "/unknown/path" as any });
    });
    expect(container.querySelector('[data-part="context-tree"]')).toBe(treeNode);
    expect(container.querySelector('[data-part="inbox"]')).toBe(inboxNode);
    unmount();
  });

  it("navigates route when selecting agent from tree inbox and manager view", async () => {
    // CONTRACT 22.5, DESIGN 3j
    // ContextTree context row click -> /a/$context
    // ContextTree plan row click -> /a/$context/$plan
    // ManagerView plan card click -> /a/$context/$plan
    // Inbox row click -> owner agent route
    const history = createMemoryHistory({ initialEntries: ["/"] });
    const router = createAppRouter({ history });

    const { container, unmount } = mount(<AppRouterView router={router} />);
    await act(async () => {
      await router.load();
    });

    // 1. Context row click in tree navigates to /a/cpd
    const contextRow = container.querySelector('[data-part="context-row"][data-context-id="cpd"]');
    expect(contextRow).not.toBeNull();
    await act(async () => {
      (contextRow as HTMLElement).click();
    });
    expect(router.state.location.pathname).toBe("/a/cpd");

    // 2. Plan row click in tree navigates to /a/cpd/plan-x
    const planRow = container.querySelector('[data-part="plan-row"][data-plan-id="plan-x"]');
    expect(planRow).not.toBeNull();
    await act(async () => {
      (planRow as HTMLElement).click();
    });
    expect(router.state.location.pathname).toBe("/a/cpd/plan-x");

    // 3. Inbox row click navigates to owner session route
    const inboxRow = container.querySelector('[data-part="inbox-row"][data-agent-key="cpd:plan-x:web-tests"]');
    expect(inboxRow).not.toBeNull();
    await act(async () => {
      (inboxRow as HTMLElement).click();
    });
    expect(router.state.location.pathname).toBe("/a/cpd/plan-x/web-tests");
    unmount();
  });

  it("restores agent route from hash history and supports back forward navigation", async () => {
    // CONTRACT 22.1, 22.6 (D2, A6)
    // createHashHistory: window.location.hash navigation and back/forward
    const hashHistory = createHashHistory();
    window.location.hash = "#/a/cpd/plan-x";

    const router = createAppRouter({ history: hashHistory });
    const { unmount } = mount(<AppRouterView router={router} />);
    await act(async () => {
      await router.load();
    });

    // Check location restored from hash
    expect(router.state.location.pathname).toBe("/a/cpd/plan-x");

    // Push new route
    await act(async () => {
      await router.navigate({ to: "/a/$context", params: { context: "cpd" } });
    });
    expect(router.state.location.pathname).toBe("/a/cpd");
    expect(window.location.hash).toBe("#/a/cpd");

    // History back
    await act(async () => {
      window.history.back();
    });
    expect(router.state.location.pathname).toBe("/a/cpd/plan-x");

    // History forward
    await act(async () => {
      window.history.forward();
    });
    expect(router.state.location.pathname).toBe("/a/cpd");
    unmount();
  });
});
