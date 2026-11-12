import React from "react";

export interface StatusBarProps {
  version?: string;
  hasUpgrade?: boolean;
  onUpgrade?: () => void;
}

export function StatusBar({
  version = "unknown",
  hasUpgrade = false,
  onUpgrade,
}: StatusBarProps): React.ReactElement {
  return (
    <footer aria-label="Status bar" className="flex items-center gap-2 p-1 text-xs">
      <span data-part="version" data-alert={hasUpgrade ? "true" : undefined}>
        {version}
        {hasUpgrade && (
          <button
            type="button"
            data-part="upgrade-button"
            onClick={onUpgrade}
            className="ml-2 text-primary underline"
          >
            Upgrade available
          </button>
        )}
      </span>
    </footer>
  );
}
