/// Service worker source: precaches dist/ (manifest injected by scripts/build-sw.mjs).
import { cleanupOutdatedCaches, matchPrecache, precacheAndRoute } from "workbox-precaching";
import { routeRequest } from "./sw-routes.ts";

interface WorkerScope {
  __WB_MANIFEST: Array<{ url: string; revision: string | null }>;
  location: { origin: string };
  skipWaiting(): Promise<void>;
  addEventListener(type: string, listener: (event: never) => void): void;
}
interface FetchLike {
  request: { method: string; url: string; mode: string };
  respondWith(r: Promise<Response | undefined>): void;
}
interface MessageLike {
  data: { type?: string } | null;
}

const scope = self as unknown as WorkerScope;

cleanupOutdatedCaches();
precacheAndRoute((self as unknown as WorkerScope).__WB_MANIFEST);

scope.addEventListener("fetch", (event: FetchLike) => {
  const { method, url, mode } = event.request;
  if (routeRequest({ method, url, mode, origin: scope.location.origin }) !== "navigation-fallback") return;
  event.respondWith(matchPrecache("/index.html").then((cached) => cached ?? fetch(event.request as unknown as Request)));
});

scope.addEventListener("message", (event: MessageLike) => {
  if (event.data?.type === "SKIP_WAITING") void scope.skipWaiting();
});
