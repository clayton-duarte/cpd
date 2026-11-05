import { existsSync, readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext, Theme } from "@earendil-works/pi-coding-agent";
import { isKeyRelease, isKeyRepeat, matchesKey, type OverlayHandle, type TUI } from "@earendil-works/pi-tui";
import {
	COLLAPSE_KEY_OFF,
	formatKeySpecForDisplay,
	loadConfig,
	resolveCollapseKey,
} from "./config.ts";
import {
	ASK_USER_BLOCKED_EVENT,
	ASK_USER_PROMPT_EVENT,
	type AskUserBlockedEventPayload,
	type AskUserPromptEventPayload,
} from "./events.ts";
import {
	mustYieldInlineSlot,
	type Presentation,
	resolvePresentation,
	rowBudgetForHost,
	STACKED_OVERLAY_PRESENTATION,
	YIELD_TO_OVERLAY,
} from "./presentation.ts";
import { MOUNT_BOUND_MS, RENDER_GUARD_TICK_MS, RenderGuard } from "./hang-guard.ts";
import { editWithExternalEditor } from "./state/external-editor.ts";
import {
	type QuestionnaireSession as QuestionnaireSessionInstance,
	type QuestionnaireSessionComponent,
	type QuestionnaireSessionConfig,
	QuestionnaireSession,
} from "./state/questionnaire-session.ts";
import { type DialogUI, hasDialogUI, runRpcQuestionnaire } from "./rpc-fallback.ts";
import { displayLabel, t } from "./state/i18n-bridge.ts";
import { sentinelsToAppend } from "./state/row-intent.ts";
import { normalizeQuestionParams, prepareQuestionArguments } from "./tool/normalize-params.ts";
import { buildQuestionnaireResponse, buildToolResult } from "./tool/response-envelope.ts";
import {
	MAX_OPTIONS,
	MAX_QUESTIONS,
	MIN_OPTIONS,
	type QuestionData,
	type QuestionnaireError,
	type QuestionnaireResult,
	type QuestionParams,
	QuestionParamsSchema,
} from "./tool/types.ts";
import { limitSentence } from "./limit-text.ts";
import { validateQuestionnaire } from "./tool/validate-questionnaire.ts";
import type { WrappingSelectItem } from "./view/components/wrapping-select.ts";

function emitAskUserPromptEvent(pi: ExtensionAPI, params: QuestionParams): void {
	const payload: AskUserPromptEventPayload = {
		questions: params.questions.map((q) => ({
			question: q.question,
			header: q.header,
			multiSelect: q.multiSelect ?? false,
			options: q.options.map((o) => ({
				label: o.label,
				description: o.description,
				hasPreview: typeof o.preview === "string" && o.preview.length > 0,
			})),
		})),
	};
	pi.events.emit(ASK_USER_PROMPT_EVENT, payload);
}

function emitAskUserBlockedEvent(pi: ExtensionAPI, active: boolean): void {
	const payload: AskUserBlockedEventPayload = { active };
	pi.events.emit(ASK_USER_BLOCKED_EVENT, payload);
}

/** Non-interactive host backstop (the reconciler normally strips the tool first). */
function rejectWithoutUi() {
	return buildToolResult(ERROR_NO_UI, { answers: [], cancelled: true, error: "no_ui" });
}

/** Sequential native-dialog walker for RPC hosts; brackets it with the blocked-event pair + terminal bell. */
async function runRpcPath(pi: ExtensionAPI, ui: DialogUI, typed: QuestionParams) {
	emitAskUserBlockedEvent(pi, true);
	try {
		emitTerminalAttention();
		return buildQuestionnaireResponse(await runRpcQuestionnaire(ui, typed), typed);
	} finally {
		emitAskUserBlockedEvent(pi, false);
	}
}

/** Canonical tool name — single source of truth shared with the reconcile module. */
export const ASK_USER_QUESTION_TOOL_NAME = "ask_user_question";

const ERROR_NO_UI = "Error: UI not available (running in non-interactive mode)";

const ERROR_NO_CUSTOM_UI =
	"Error: this client cannot render the questionnaire (custom UI is unavailable, e.g. RPC/ACP hosts such as Zed or Paseo). The user never saw the questions — do NOT treat this as a decline. Ask the questions as plain chat text instead, without using this tool.";

/** Model-facing sentences of the never-hang resolutions, verbatim from docs/commands.md §15.13. */
const DISPLACED_SENTENCE =
	"Error: the question dialog disappeared before the user answered (another dialog took the single dialog slot). The user never saw the questions — do NOT treat this as a decline. Ask the questions as plain chat text instead, without using this tool.";

const STUCK_SENTENCE =
	"Error: the question dialog did not settle within 2 hours and was closed. The user never saw the questions — do NOT treat this as a decline. Ask the questions as plain chat text instead, without using this tool.";

/** Loud, coded result when the inline slot was taken by another dialog (§15.13). */
const DISPLACED_RESULT: QuestionnaireResult = Object.freeze({
	answers: [],
	cancelled: true,
	error: "dialog_displaced",
});

/** Loud, coded result when a mount outlives the two-hour bound (§15.13). */
const STUCK_RESULT: QuestionnaireResult = Object.freeze({
	answers: [],
	cancelled: true,
	error: "dialog_stuck",
});

/** Standard terminal bell — same byte rpiv-warp exports as OSC_TERMINATOR. */
export const BEL = "\x07";

/**
 * Emit one portable terminal attention signal without touching redirected output.
 * Writes to stdout rather than rpiv-warp's `/dev/tty` transport: the `isTTY` gate
 * both proves an interactive terminal owns the coming wait and keeps the byte out
 * of piped RPC transports (VS Code pendant, Zed) — a `/dev/tty` write would ring
 * even when the questionnaire renders in a remote host's own UI.
 */
function emitTerminalAttention(): void {
	try {
		if (process.stdout.isTTY) process.stdout.write(BEL);
	} catch {
		// Terminal attention is best effort; the questionnaire must still proceed.
	}
}

type SessionRef = { current: QuestionnaireSessionInstance | null };
type OverlayHandleRef = { current: OverlayHandle | undefined };
/** The inline mount's render guard, registered at mount time so the mount teardown can stop it. */
type GuardRef = { current: RenderGuard | undefined };
/** The `done` the host handed the component factory, for the two-hour bound. */
type DoneRef = { current: ((result: QuestionnaireResult) => void) | undefined };

/**
 * Register the raw terminal listener that toggles collapse while the overlay is hidden.
 * Returns the remover, or undefined when the key is off / the host has no raw input hook —
 * callers derive `canReopenWhileHidden` from that.
 */
function registerCollapseKeyListener(
	ctx: ExtensionContext,
	collapseKey: string,
	sessionRef: SessionRef,
	overlayHandleRef: OverlayHandleRef,
): (() => void) | undefined {
	if (collapseKey === COLLAPSE_KEY_OFF || typeof ctx.ui.onTerminalInput !== "function") return undefined;
	let hasAnnouncedHide = false;
	return ctx.ui.onTerminalInput((data) => {
		const handle = overlayHandleRef.current;
		if (!handle) return undefined;
		// Only act while the questionnaire is hidden (its handleInput is
		// unreachable) or actually focused. When some other overlay is on
		// top (e.g. `/btw`), leave the keystroke to that overlay instead of
		// toggling the questionnaire from underneath it.
		if (!handle.isHidden() && !handle.isFocused()) return undefined;
		if (!matchesKey(data, collapseKey as Parameters<typeof matchesKey>[1])) return undefined;
		// Kitty-protocol terminals report press, repeat, and release separately.
		// Toggle only on the initial press so a tap does not immediately reopen
		// the overlay and a held key does not toggle it repeatedly.
		if (isKeyRelease(data) || isKeyRepeat(data)) return { consume: true };
		sessionRef.current?.toggleCollapsedExternal();
		if (handle.isHidden() && !hasAnnouncedHide) {
			hasAnnouncedHide = true;
			ctx.ui.notify?.(`ask_user_question hidden — press ${formatKeySpecForDisplay(collapseKey)} to reopen`, "info");
		}
		return { consume: true };
	});
}

/**
 * Build the `ctx.ui.custom` component factory: constructs the session (capturing it in
 * `sessionRef`) and exposes its component. `editInput` keeps the host package a dynamic import —
 * the host package stays a dynamic import.
 */
function makeSessionFactory(config: {
	ctx: ExtensionContext;
	typed: QuestionParams;
	itemsByTab: WrappingSelectItem[][];
	collapseKey: string;
	canReopenWhileHidden: boolean;
	presentation: Presentation;
	sessionRef: SessionRef;
	Session: typeof QuestionnaireSession;
	guardRef: GuardRef;
	doneRef: DoneRef;
}) {
	const {
		ctx,
		typed,
		itemsByTab,
		collapseKey,
		canReopenWhileHidden,
		presentation,
		sessionRef,
		Session,
		guardRef,
		doneRef,
	} = config;
	return (
		tui: TUI,
		theme: Theme,
		keybindings: QuestionnaireSessionConfig["keybindings"],
		done: (result: QuestionnaireResult) => void,
	): QuestionnaireSessionComponent => {
		// The factory runs at mount time — the first point the TUI is reachable — so this is
		// where an inline mount notices an overlay already on screen and gives the slot up.
		if (!presentation.overlay && mustYieldInlineSlot(tui)) {
			done(YIELD_TO_OVERLAY);
			return INERT_COMPONENT;
		}
		const session = new Session({
			tui,
			theme,
			params: typed,
			itemsByTab,
			done,
			keybindings,
			editInput: async (value) => {
				try {
					const { SettingsManager } = await import("@earendil-works/pi-coding-agent");
					const editorCommand = SettingsManager.create(ctx.cwd, undefined, {
						projectTrusted: ctx.isProjectTrusted(),
					}).getExternalEditorCommand();
					if (!editorCommand) throw new Error("No external editor command is configured");
					return await editWithExternalEditor(tui, editorCommand, value);
				} catch (error) {
					const message = error instanceof Error ? error.message : String(error);
					ctx.ui.notify(`${t("editor.failed", "External editor failed")}: ${message}`, "error");
					return undefined;
				}
			},
			collapseKey,
			canReopenWhileHidden,
			rowBudget: rowBudgetForHost(tui.terminal, presentation.rowBudget),
		});
		sessionRef.current = session;
		doneRef.current = done;
		const component = session.component;
		// The render guard applies to the inline mount only: overlays stack over other
		// overlays and cannot be displaced, so they are left unwrapped.
		if (presentation.overlay) return component;
		const guard = new RenderGuard(tui, component, () => done(DISPLACED_RESULT));
		guardRef.current = guard;
		guard.start(RENDER_GUARD_TICK_MS);
		return guard.component;
	};
}

/** Stand-in returned by a mount that resolved before mounting (Pi discards it unmounted). */
const INERT_COMPONENT: QuestionnaireSessionComponent = {
	render: () => [],
	invalidate: () => {},
	handleInput: () => {},
};

/**
 * One `ctx.ui.custom` mount under `presentation`. The overlay additionally gets its handle
 * captured (so collapse can `setHidden()`) and a raw terminal listener for the collapse key,
 * which stays reachable while the overlay is hidden (pi-tui routes no input to a hidden
 * overlay's `component.handleInput`). Inline has no handle to hide and keeps focus while
 * collapsed, so it needs no listener.
 */
async function mountQuestionnaire(config: {
	ctx: ExtensionContext;
	typed: QuestionParams;
	itemsByTab: WrappingSelectItem[][];
	collapseKey: string;
	presentation: Presentation;
	Session: typeof QuestionnaireSession;
}): Promise<QuestionnaireResult | undefined> {
	const { ctx, collapseKey, presentation } = config;
	const sessionRef: SessionRef = { current: null };
	const overlayHandleRef: OverlayHandleRef = { current: undefined };
	const guardRef: GuardRef = { current: undefined };
	const doneRef: DoneRef = { current: undefined };
	const removeOverlayInputListener = presentation.hideOnCollapse
		? registerCollapseKeyListener(ctx, collapseKey, sessionRef, overlayHandleRef)
		: undefined;
	// Hiding the overlay is only reversible through the raw listener above, so the session
	// may emit `setHidden` only when it was actually registered; otherwise collapse falls
	// back to the visible one-line row.
	const canReopenWhileHidden = removeOverlayInputListener !== undefined;
	// Every mount, inline and overlay, is bounded at MOUNT_BOUND_MS: a mount that neither
	// settles nor is displaced resolves loud with the stuck result. The bound wins its race
	// with the host's custom() promise even when that promise never settles (the displaced
	// slot, a stuck session) — the custom promise is abandoned; the guard is cleared first.
	let boundTimer: ReturnType<typeof setTimeout> | undefined;
	let externalTimer: ReturnType<typeof setInterval> | undefined;
	const cpdSession = process.env.CPD_SESSION;
	const answerFile = cpdSession
		? join(process.env.CPD_HOME || join(homedir(), ".cpd"), "run", "ask", `${cpdSession}.json`)
		: undefined;
	if (answerFile) {
		try {
			rmSync(answerFile, { force: true });
		} catch {
			// ignore
		}
	}
	const bound = new Promise<QuestionnaireResult>((resolve) => {
		boundTimer = setTimeout(() => {
			guardRef.current?.stop();
			doneRef.current?.(STUCK_RESULT);
			resolve(STUCK_RESULT);
		}, MOUNT_BOUND_MS);
		if (answerFile) {
			externalTimer = setInterval(() => {
				try {
					if (!existsSync(answerFile)) return;
					const raw = readFileSync(answerFile, "utf8");
					rmSync(answerFile, { force: true });
					const parsed = JSON.parse(raw) as { result?: QuestionnaireResult };
					if (parsed?.result) {
						guardRef.current?.stop();
						doneRef.current?.(parsed.result);
						resolve(parsed.result);
					}
				} catch {
					// ignore transient read/parse errors
				}
			}, 120);
			externalTimer.unref?.();
		}
	});
	try {
		return await Promise.race([
			ctx.ui.custom<QuestionnaireResult>(
				makeSessionFactory({ ...config, canReopenWhileHidden, sessionRef, guardRef, doneRef }),
				presentation.overlay
					? {
							overlay: true,
							overlayOptions: {
								anchor: "bottom-center",
								width: "100%",
								maxHeight: "100%",
								margin: { left: 0, right: 0, bottom: 0 },
							},
							onHandle: (handle) => {
								overlayHandleRef.current = handle;
								sessionRef.current?.setOverlayHandle(handle);
							},
						}
					: { overlay: false },
			),
			bound,
		]);
	} finally {
		clearTimeout(boundTimer);
		clearInterval(externalTimer);
		if (answerFile) {
			try {
				rmSync(answerFile, { force: true });
			} catch {
				// ignore
			}
		}
		guardRef.current?.stop();
		removeOverlayInputListener?.();
	}
}

/**
 * A TUI questionnaire ALWAYS resolves a QuestionnaireResult (cancel included), so
 * `undefined` uniquely means "host cannot render", never "user declined". RPC builds
 * that predate ctx.mode land here: run the dialog walker when the host has the
 * primitives; otherwise tell the model the user never saw the questions.
 */
async function resolveUndefinedResult(ctx: ExtensionContext, typed: QuestionParams) {
	if (hasDialogUI(ctx.ui)) {
		return buildQuestionnaireResponse(await runRpcQuestionnaire(ctx.ui, typed), typed);
	}
	return buildToolResult(ERROR_NO_CUSTOM_UI, { answers: [], cancelled: true, error: "no_custom_ui" });
}

export function buildItemsForQuestion(question: QuestionData): WrappingSelectItem[] {
	const items: WrappingSelectItem[] = question.options.map((o) => ({
		kind: "option",
		label: o.label,
		description: o.description,
	}));
	for (const kind of sentinelsToAppend(question)) {
		items.push({ kind, label: displayLabel(kind) });
	}
	return items;
}

export const DEFAULT_PROMPT_SNIPPET = `Ask the user up to ${MAX_QUESTIONS} structured questions (${MIN_OPTIONS}-${MAX_OPTIONS} options each) when requirements are ambiguous`;
export const DEFAULT_PROMPT_GUIDELINES: string[] = [
	`Use ask_user_question whenever the user's request is underspecified and you cannot proceed without concrete decisions — you can ask up to ${MAX_QUESTIONS} questions per invocation.`,
	`Each question MUST have ${MIN_OPTIONS}-${MAX_OPTIONS} options. Every option requires a concise label (1-5 words) and a description explaining what the choice means or its trade-offs. The user can additionally type a custom answer via the automatically appended "Type something." row on every question, or press Esc to abandon the questionnaire. Do NOT author "Other" or "Type something." labels yourself — reserved labels are rejected at runtime.`,
	`Set multiSelect: true when multiple answers are valid. Provide an options[].preview markdown string when an option benefits from richer side-by-side context (mockups, code snippets, diagrams, configs) — single-select only. The "Type something." row is appended to every question; in preview mode it expands to the full pane width while typing so the custom answer is not cramped into the narrow options column. If you recommend a specific option, make that the first option and append "(Recommended)" to its label.`,
	"Do not stack multiple ask_user_question calls back-to-back — group all clarifying questions into one invocation.",
	limitSentence,
];

export const DEFAULT_TOOL_DESCRIPTION = `Ask the user one or more structured questions during execution. Use when you need to:
1. Gather user preferences or requirements
2. Clarify ambiguous instructions
3. Get decisions on implementation choices as you work
4. Offer choices to the user about what direction to take

Usage notes:
- Users can type a custom answer via the automatically appended "Type something." row on every question or press Esc to abandon the questionnaire. Do NOT author "Other" or "Type something." labels yourself — reserved labels are rejected at runtime.
- Use multiSelect: true when multiple answers are valid. The "Type something." row is available on every question, including when options carry a \`preview\`; in preview mode it expands to the full pane width while typing so the custom answer is not cramped into the narrow options column.
- If you recommend a specific option, make that the first option in the list and add "(Recommended)" at the end of the label.

Preview feature:
Use the optional \`preview\` field on options when presenting concrete artifacts that users need to visually compare:
- ASCII mockups of UI layouts or components
- Code snippets showing different implementations
- Diagram variations
- Configuration examples

Preview content is rendered as markdown in a monospace box. Multi-line text with newlines is supported. When any option has a preview, the UI switches to a side-by-side layout with a vertical option list on the left and preview on the right. Do not use previews for simple preference questions where labels and descriptions suffice. Note: previews are only supported for single-select questions (not multiSelect).\n\n${limitSentence}`;

export function registerAskUserQuestionTool(pi: ExtensionAPI): void {
	pi.registerTool({
		name: ASK_USER_QUESTION_TOOL_NAME,
		// Declared to the model but never callable from a script, because pi's codemode mode "only" hides every direct tool.
		exposure: "model-only",
		label: "Ask User Question",
		description: DEFAULT_TOOL_DESCRIPTION,
		promptSnippet: DEFAULT_PROMPT_SNIPPET,
		promptGuidelines: DEFAULT_PROMPT_GUIDELINES,
		parameters: QuestionParamsSchema,
		// Undo double-encoded nested arguments before Pi validates them (#244).
		prepareArguments: prepareQuestionArguments,

		async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
			// Line-terminator normalization runs once here, ahead of validation, so
			// every downstream consumer — validator, TUI, RPC walker, envelope, prompt
			// event — sees the same clean text (#192).
			const typed = normalizeQuestionParams(params as unknown as QuestionParams);
			if (!ctx.hasUI) return rejectWithoutUi();

			const validation = validateQuestionnaire(typed);
			if (!validation.ok) {
				return buildToolResult(validation.message, {
					answers: [],
					cancelled: true,
					error: validation.error,
				});
			}

			// Emit event for external listeners (e.g., notification plugins)
			emitAskUserPromptEvent(pi, typed);

			// RPC hosts (VSCode pendant, ACP clients like Zed/Paseo — issue #78):
			// ui.custom() cannot render there, but the select/input dialog
			// sub-protocol works. Hosts that advertise ctx.mode (pi ≥0.79) route to
			// the sequential dialog walker up front, skipping the TUI render-graph
			// import entirely; RPC builds that predate ctx.mode are caught by the
			// custom()-resolved-undefined backstop below. See ./rpc-fallback.ts.
			if ((ctx as { mode?: string }).mode === "rpc" && hasDialogUI(ctx.ui)) {
				return runRpcPath(pi, ctx.ui, typed);
			}

			const itemsByTab: WrappingSelectItem[][] = typed.questions.map((q) => buildItemsForQuestion(q));

			// Resolve the collapse/expand key spec from config. Default is `ctrl+]`; users
			// with non-US layouts (e.g. Latin American, where `]` is shifted) can override
			// via the `collapseKey` config field. `resolveCollapseKey` also accepts the
			// sentinel value `"off"` to disable the shortcut entirely.
			const collapseKey = resolveCollapseKey(loadConfig());
			// Pi's editor slot (#253); the overlay only while another questionnaire holds the slot,
			// or stacked when another overlay is already on screen (see below).
			let presentation = resolvePresentation();
			const mount = (p: Presentation) =>
				mountQuestionnaire({ ctx, typed, itemsByTab, collapseKey, presentation: p, Session: QuestionnaireSession });

			emitAskUserBlockedEvent(pi, true);
			try {
				emitTerminalAttention();
				let result = await mount(presentation);
				if (result === YIELD_TO_OVERLAY) {
					// Another overlay was on screen: stack on top of it instead (see mustYieldInlineSlot).
					presentation.release?.();
					presentation = STACKED_OVERLAY_PRESENTATION;
					result = await mount(presentation);
				}

				if (result === undefined) {
					return resolveUndefinedResult(ctx, typed);
				}

				// The never-hang resolutions (§15.13): the mount settled because the inline
				// slot was displaced or the two-hour bound elapsed. Either way the user never
				// saw the questions — resolve loud (isError, coded details) so the model
				// re-asks them as plain chat text instead of treating it as a decline.
				if (result.error === "dialog_displaced") {
					return { ...buildToolResult(DISPLACED_SENTENCE, result), isError: true };
				}
				if (result.error === "dialog_stuck") {
					return { ...buildToolResult(STUCK_SENTENCE, result), isError: true };
				}

				return buildQuestionnaireResponse(result, typed);
			} finally {
				presentation.release?.();
				emitAskUserBlockedEvent(pi, false);
			}
		},
	});
}

export { buildQuestionnaireResponse, buildToolResult };
