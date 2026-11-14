import React from "react";
import type { Banner } from "../state/banner.ts";

const TEXT: Record<Banner["kind"], string> = {
  "reconnecting": "Reconnecting to the server",
  "server-unreachable": "Server unreachable",
  "engine-down": "Engine is down",
  "engine-restarting": "Engine is restarting for an install",
};

/** The single banner of the shell: the socket's state, or the engine's while the socket is open. */
export function EngineBanner({ banner }: { banner: Banner | null }): React.ReactElement | null {
  if (banner === null) return null;
  return (
    <div
      role="status"
      data-part="engine-banner"
      data-kind={banner.kind}
      className="border-b border-destructive/20 bg-destructive/10 px-3 py-1 text-sm text-destructive"
    >
      {TEXT[banner.kind]}
      {banner.since === null ? "" : ` since ${banner.since}`}
    </div>
  );
}
