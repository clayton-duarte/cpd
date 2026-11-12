import React from "react";

export interface UpgradeBannerProps {
  step?: string;
  code?: string;
  backup?: string | null;
  rolledBack?: boolean;
  nextStep?: string;
}

export function UpgradeBanner({
  step,
  code,
  backup,
  rolledBack = true,
  nextStep,
}: UpgradeBannerProps): React.ReactElement {
  if (rolledBack) {
    return (
      <div data-part="upgrade-banner" data-state="rolled-back" className="bg-destructive/10 border-b border-destructive/20 p-2 text-sm text-destructive">
        <span>Upgrade failed at step {step ?? "unknown"}{code ? ` (${code})` : ""}. Host was restored from backup {backup ?? "unknown"}.</span>
      </div>
    );
  }

  return (
    <div data-part="upgrade-banner" data-state="rollback-failed" className="bg-destructive border-b border-destructive/50 p-2 text-sm text-destructive-foreground">
      <span>Upgrade failed at step {step ?? "unknown"}{code ? ` (${code})` : ""} and rollback failed. Host is in an inconsistent state. Next step: {nextStep ?? "manual repair required"}.</span>
    </div>
  );
}
