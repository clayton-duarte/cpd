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

export function ResumeButton(props: ResumeButtonProps): ReactElement | null {
  const { sessionKey, state, cpd } = props;
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  if (state !== "stopped") {
    return null;
  }

  const handleResume = async (): Promise<void> => {
    if (!cpd) return;
    setLoading(true);
    setError(null);
    try {
      await resumeSession(cpd, sessionKey);
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex items-center gap-2">
      <Button
        type="button"
        data-action="resume"
        disabled={loading}
        onClick={handleResume}
      >
        Resume
      </Button>
      {error !== null ? (
        <span data-part="resume-error" className="text-sm text-destructive">
          {error}
        </span>
      ) : null}
    </div>
  );
}
