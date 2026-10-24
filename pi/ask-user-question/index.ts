/**
 * rpiv-ask-user-question — Pi extension. Registers the `ask_user_question`
 * tool: a structured option selector with an automatically appended
 * `Type something.` custom-answer row.
 *
 * cpd's copy has no i18n: `t(key, fallback)` of the bridge returns the English fallback.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerAskUserQuestionTool } from "./ask-user-question.ts";
import { findOtherCopy, standDown } from "./double-load.ts";
import { registerAskUserQuestionReconciler } from "./reconcile.ts";

export {
	ASK_USER_BLOCKED_EVENT,
	ASK_USER_PROMPT_EVENT,
	type AskUserBlockedEventPayload,
	type AskUserPromptEventPayload,
	type AskUserPromptOption,
	type AskUserPromptQuestion,
} from "./events.ts";

export default function (pi: ExtensionAPI) {
	// Only a Lead (CPD_SESSION and CPD_PLAN) or a Manager (CPD_SESSION alone) has this tool; a Worker has CPD_ATTEMPT too.
	if (!process.env.CPD_SESSION || process.env.CPD_ATTEMPT) return;
	const found = findOtherCopy(process.cwd());
	if (found) return standDown(pi, found);
	registerAskUserQuestionTool(pi);
	registerAskUserQuestionReconciler(pi);
}
