import { useState } from "react";
import type { ReactElement } from "react";
import { Button } from "../components/button.tsx";
import { resumeSession } from "../../server/cockpit/resume.ts";
import type { Cpd } from "../../server/cockpit/cli.ts";

export interface ResumeButtonProps {
  readonly sessionKey: string;
  readonly state?: string;
  readonly cpd?: Cpd;
}

export function ResumeButton(_props: ResumeButtonProps): ReactElement | null {
  // Stub implementation so tests fail
  return null;
}
