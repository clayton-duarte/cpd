import type { ReactElement } from "react";

export interface ResumeButtonProps {
  readonly sessionKey: string;
  readonly state?: string;
  readonly onResume?: () => Promise<void> | void;
}

export function ResumeButton(_props: ResumeButtonProps): ReactElement | null {
  return null;
}
