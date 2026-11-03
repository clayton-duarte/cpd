import type { Dialog, DialogResponse } from "../../protocol/domain.ts";

export function PlanReviewCard(_props: {
  dialog: Extract<Dialog, { method: "cpd_plan_review" }>;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
}): React.JSX.Element {
  throw new Error("not implemented");
}
