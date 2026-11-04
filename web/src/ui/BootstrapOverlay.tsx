import { useEffect, useState } from "react";
import type { ReactElement } from "react";
import type { UpdateBridge } from "../electron/channels.ts";
import type { BootstrapStatus } from "../electron/update/types.ts";
import { bootstrapView } from "./state/bootstrap-view.ts";

function updateBridge(): UpdateBridge | null {
  if (typeof window === "undefined") return null;
  const bridge = (window as unknown as { cpdElectron?: Partial<UpdateBridge> }).cpdElectron;
  return bridge?.onUpdateBootstrap === undefined ? null : (bridge as UpdateBridge);
}

export function BootstrapOverlay(): ReactElement | null {
  const [status, setStatus] = useState<BootstrapStatus>({ state: "ready" });
  useEffect(() => updateBridge()?.onUpdateBootstrap(setStatus), []);
  const view = bootstrapView(status);
  if (!view.visible) return null;
  return (
    <div role="alert" data-part="bootstrap-overlay" className="fixed inset-0 z-50 grid place-items-center bg-background/90 text-foreground">
      <div className="max-w-md rounded-lg border border-border bg-card p-6 text-center">
        <h2 className="text-fg-strong">{view.title}</h2>
        <p className="mt-2 text-muted-foreground">{view.detail}</p>
      </div>
    </div>
  );
}
