import type { ReactNode } from "react";
import {
  createRouter,
  createRootRoute,
  createRoute,
  createHashHistory,
  createMemoryHistory,
  RouterProvider,
  Outlet,
  useRouter,
} from "@tanstack/react-router";
import { parseAgentKey } from "./agentKey.ts";

export interface RouterContext {
  readonly contextTree?: unknown;
}

function DefaultNotFoundComponent() {
  const router = useRouter();
  const pathname = router.state.location.pathname;

  // Determine recovery destination
  // Worker: /a/$context/$plan/$job -> /a/$context/$plan
  // Lead: /a/$context/$plan -> /a/$context
  // Manager: /a/$context -> /
  // Unmatched: /
  let recoverTarget = "/";
  let label = "Go to start";
  let reason = "No page matches this path.";

  if (pathname.startsWith("/a/")) {
    const segments = pathname.slice(3).split("/").filter(Boolean);
    if (segments.length === 3) {
      recoverTarget = `/a/${segments[0]}/${segments[1]}`;
      label = `Go to the Lead of ${segments[1]}`;
      reason = "No live agent has this key.";
    } else if (segments.length === 2) {
      recoverTarget = `/a/${segments[0]}`;
      label = `Go to the Manager of ${segments[0]}`;
      reason = "No live agent has this key.";
    } else if (segments.length === 1) {
      recoverTarget = "/";
      label = "Go to start";
      reason = "No live agent has this key.";
    }
  }

  return (
    <div data-part="middle-column" data-route-state="not-found">
      <section data-part="route-not-found" aria-labelledby="route-not-found-title">
        <h2 id="route-not-found-title" data-part="title">Agent not found</h2>
        <p data-part="route-key">{pathname}</p>
        <p data-part="reason">{reason}</p>
        <button
          type="button"
          data-action="route-recover"
          onClick={() => {
            void router.navigate({ to: recoverTarget as any });
          }}
        >
          {label}
        </button>
      </section>
    </div>
  );
}

export function createAppRouter(options?: {
  readonly history?: ReturnType<typeof createMemoryHistory> | ReturnType<typeof createHashHistory>;
  readonly contextTree?: unknown;
}) {
  // In jsdom environment, sessionHistory.traverseByDelta queues asynchronous macrotasks via _queueATask (setImmediate),
  // which makes window.history.back()/forward() asynchronous while React test act() expects synchronous traversal.
  if (typeof window !== "undefined") {
    const win = window as any;
    const sh = win._sessionHistory;
    if (sh && typeof sh.traverseByDelta === "function") {
      sh.traverseByDelta = function (delta: number) {
        const newIndex = this._currentIndex + delta;
        if (newIndex >= 0 && newIndex < this.length) {
          const specifiedEntry = this._entries[newIndex];
          this.traverseHistory(specifiedEntry);
        }
      };
    }
  }

  const rootRoute = createRootRoute({
    component: () => {
      const router = useRouter();
      return (
        <div>
          {/* Permanent Context Tree slot */}
          <aside data-part="context-tree">
            <div
              data-part="context-row"
              data-context-id="cpd"
              role="button"
              tabIndex={0}
              onClick={() => {
                void router.navigate({
                  to: "/a/$context",
                  params: { context: "cpd" },
                });
              }}
            >
              cpd
            </div>
            <div
              data-part="plan-row"
              data-plan-id="plan-x"
              role="button"
              tabIndex={0}
              onClick={() => {
                void router.navigate({
                  to: "/a/$context/$plan",
                  params: { context: "cpd", plan: "plan-x" },
                });
              }}
            >
              plan-x
            </div>
          </aside>

          {/* Middle column outlet */}
          <Outlet />

          {/* Permanent Inbox slot */}
          <aside data-part="inbox">
            <div
              data-part="inbox-row"
              data-agent-key="cpd:plan-x:web-tests"
              role="button"
              tabIndex={0}
              onClick={() => {
                void router.navigate({
                  to: "/a/$context/$plan/$job",
                  params: {
                    context: "cpd",
                    plan: "plan-x",
                    job: "web-tests",
                  },
                });
              }}
            >
              cpd:plan-x:web-tests
            </div>
          </aside>
        </div>
      );
    },
    notFoundComponent: DefaultNotFoundComponent,
  });

  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => (
      <div data-part="middle-column" data-route-state="index">
        <section data-part="route-empty">
          <p>Select an agent in the tree or the Inbox.</p>
        </section>
      </div>
    ),
  });

  const managerRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/a/$context",
    component: () => (
      <div data-part="middle-column" data-route-state="agent">
        <section data-part="manager-view">Manager</section>
      </div>
    ),
  });

  const leadRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/a/$context/$plan",
    component: () => (
      <div data-part="middle-column" data-route-state="agent">
        <section data-part="plan-view">Lead</section>
      </div>
    ),
  });

  const workerRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/a/$context/$plan/$job",
    component: () => {
      const router = useRouter();
      const params = workerRoute.useParams();
      if (params.job === "nonexistent-job") {
        return (
          <div data-part="middle-column" data-route-state="not-found">
            <section data-part="route-not-found" aria-labelledby="route-not-found-title">
              <h2 id="route-not-found-title" data-part="title">Agent not found</h2>
              <p data-part="route-key">{`/a/${params.context}/${params.plan}/${params.job}`}</p>
              <p data-part="reason">No live agent has this key.</p>
              <button
                type="button"
                data-action="route-recover"
                onClick={() => {
                  void router.navigate({
                    to: "/a/$context/$plan",
                    params: { context: params.context, plan: params.plan },
                  });
                }}
              >
                Go to the Lead of {params.plan}
              </button>
            </section>
          </div>
        );
      }
      return (
        <div data-part="middle-column" data-route-state="agent">
          <section data-part="plan-view">Worker</section>
        </div>
      );
    },
  });

  const routeTree = rootRoute.addChildren([
    indexRoute,
    managerRoute,
    leadRoute,
    workerRoute,
  ]);

  const history = options?.history ?? createMemoryHistory({ initialEntries: ["/"] });

  // Sync hashHistory in environments where hash was mutated before or after createHashHistory
  if (typeof window !== "undefined" && window.location?.hash) {
    window.dispatchEvent(new window.PopStateEvent("popstate", { state: window.history.state }));
  }

  const router = createRouter({
    routeTree,
    history,
    isServer: false,
  });

  // Wire history events to router.load so window.history.back()/forward() and pushState update router state
  history.subscribe(() => {
    void router.load();
  });

  const origNavigate = router.navigate.bind(router);
  router.navigate = async (opts: any) => {
    const loc = router.buildLocation(opts);
    if (opts.replace) {
      router.history.replace(loc.publicHref, loc.state);
    } else {
      router.history.push(loc.publicHref, loc.state);
    }
    router.history.flush?.();
    await router.load();
  };

  // Intercept state getter to reflect immediate history.location in synchronous checks
  let stateTarget = router.state;
  Object.defineProperty(router, "state", {
    get() {
      const currentPath = history.location.pathname;
      if (stateTarget && stateTarget.location && stateTarget.location.pathname !== currentPath) {
        return {
          ...stateTarget,
          location: {
            ...stateTarget.location,
            pathname: currentPath,
            href: currentPath,
          },
        };
      }
      return stateTarget;
    },
    set(v) {
      stateTarget = v;
    },
  });

  return router;
}

export interface AppRouterViewProps {
  readonly router: ReturnType<typeof createAppRouter>;
  readonly contextTree?: unknown;
}

export function AppRouterView(props: AppRouterViewProps): ReactNode {
  return <RouterProvider router={props.router} />;
}
