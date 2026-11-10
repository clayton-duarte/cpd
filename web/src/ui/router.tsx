import type { ReactNode } from "react";
import {
  createRouter,
  createRootRoute,
  createRoute,
  createHashHistory,
  createMemoryHistory,
} from "@tanstack/react-router";

export interface RouterContext {
  readonly contextTree?: unknown;
}

export function createAppRouter(options?: {
  readonly history?: ReturnType<typeof createMemoryHistory> | ReturnType<typeof createHashHistory>;
  readonly contextTree?: unknown;
}) {
  const rootRoute = createRootRoute({
    component: () => {
      throw new Error("stub: RootRouteComponent not implemented");
    },
    notFoundComponent: () => {
      throw new Error("stub: RootNotFoundComponent not implemented");
    },
  });

  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => {
      throw new Error("stub: IndexRouteComponent not implemented");
    },
  });

  const managerRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/a/$context",
    component: () => {
      throw new Error("stub: ManagerRouteComponent not implemented");
    },
  });

  const leadRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/a/$context/$plan",
    component: () => {
      throw new Error("stub: LeadRouteComponent not implemented");
    },
  });

  const workerRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/a/$context/$plan/$job",
    component: () => {
      throw new Error("stub: WorkerRouteComponent not implemented");
    },
  });

  const routeTree = rootRoute.addChildren([
    indexRoute,
    managerRoute,
    leadRoute,
    workerRoute,
  ]);

  return createRouter({
    routeTree,
    history: options?.history ?? createMemoryHistory({ initialEntries: ["/"] }),
  });
}

export interface AppRouterViewProps {
  readonly router: ReturnType<typeof createAppRouter>;
  readonly contextTree?: unknown;
}

export function AppRouterView(props: AppRouterViewProps): ReactNode {
  throw new Error("stub: AppRouterView not implemented");
}
