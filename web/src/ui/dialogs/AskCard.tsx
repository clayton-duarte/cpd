import type { Dialog, DialogResponse } from "../../protocol/domain.ts";

export function AskCard(_props: {
  dialog: Extract<Dialog, { method: "ask_user_question" }>;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
}): React.JSX.Element {
  throw new Error("not implemented");
}
