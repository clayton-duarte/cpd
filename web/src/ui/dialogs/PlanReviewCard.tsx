import { useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Dialog, DialogResponse } from "../../protocol/domain.ts";

export function PlanReviewCard(props: {
  dialog: Extract<Dialog, { method: "cpd_plan_review" }>;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
}): React.JSX.Element {
  const { dialog, readOnly, onRespond } = props;
  const [asking, setAsking] = useState(false);
  const [feedback, setFeedback] = useState("");
  const respond = (outcome: "approve" | "reject") => onRespond({ method: "cpd_plan_review", outcome });
  const trimmed = feedback.trim();

  return (
    <section aria-label="Plan review" data-dialog="cpd_plan_review">
      <h3>{dialog.title}</h3>
      <Markdown remarkPlugins={[remarkGfm]}>{dialog.summary}</Markdown>
      {readOnly ? null : (
        <div>
          <button type="button" data-action="approve" onClick={() => respond("approve")}>
            Approve
          </button>
          <button type="button" data-action="request-changes" onClick={() => setAsking(true)}>
            Request changes
          </button>
          <button type="button" data-action="reject" onClick={() => respond("reject")}>
            Reject
          </button>
          {asking ? (
            <div>
              <textarea aria-label="Feedback" value={feedback} onChange={(e) => setFeedback(e.target.value)} />
              <button
                type="button"
                data-action="send-feedback"
                disabled={trimmed === ""}
                onClick={() => onRespond({ method: "cpd_plan_review", outcome: "changes", feedback: trimmed })}
              >
                Send feedback
              </button>
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}
