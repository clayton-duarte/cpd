export interface SwEnv {
  isElectron: boolean;
  isDev: boolean;
  isSecureContext: boolean;
  serviceWorker?: { register(url: string, opts: { scope: string }): Promise<unknown> };
}

/** Registers /sw.js at scope / in a secure production browser; Electron and dev register none. */
export async function registerServiceWorker(env: SwEnv): Promise<"registered" | "skipped"> {
  if (env.isElectron || env.isDev || !env.isSecureContext || env.serviceWorker === undefined) return "skipped";
  await env.serviceWorker.register("/sw.js", { scope: "/" });
  return "registered";
}
