export interface SwEnv {
  isElectron: boolean;
  isDev: boolean;
  isSecureContext: boolean;
  serviceWorker?: { register(url: string, opts: { scope: string }): Promise<unknown> };
}

// Stub: implemented by a later Job.
export async function registerServiceWorker(_env: SwEnv): Promise<"registered" | "skipped"> {
  return "skipped";
}
