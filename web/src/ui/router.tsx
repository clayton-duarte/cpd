import type { ReactNode } from "react";
import {
  createRouter,
  createRootRoute,
  createRoute,
  createHashHistory,
  createMemoryHistory,
} from "@tanstack/react-router";

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

  const routeTree = rootRoute.addChildren([indexRoute]);

  return createRouter({
    routeTree,
    history: options?.history ?? createMemoryHistory({ initialEntries: ["/"] }),
  });
}

export function AppRouterView(props: { readonly router: ReturnType<typeof createAppRouter> }): ReactNode {
  throw new Error("stub: AppRouterView not implemented");
}
