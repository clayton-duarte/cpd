import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences, truncateToWidth } from "@earendil-works/pi-tui";
import { onTick } from "./compact-tools-clock.ts";
import { hookEditor, setWorkingWords } from "./compact-tools-editor.ts";
import { pill, RUNNING, workingWords } from "./compact-tools-indices.ts";
import { LOADER_FRAMES, LOADER_INTERVAL_MS } from "./compact-tools-loader.ts";
import type { ToolArgs } from "./compact-tools-types.ts";


const TOOL_PROGRESS_MESSAGES: Readonly<Record<string, string>> = {
	read: "Reading file…",
	write: "Writing file…",
	edit: "Editing file…",
	bash: "Running command…",
	powershell: "Running command…",
	grep: "Searching text…",
	find: "Finding files…",
	ls: "Listing files…",
};

function humanizeToolName(toolName: string, args: ToolArgs): string {
	const routedName = (toolName === "mcp" || toolName.startsWith("mcp__")) && typeof args.tool === "string"
		? args.tool
		: toolName;
	const readableName = stripTerminalSequences(routedName)
		.replace(/[\u0000-\u001f\u007f]/gu, " ")
		.replace(/^mcp__/iu, "")
		.replace(/[_-]mcp$/iu, "")
		.replace(/([A-Z]+)([A-Z][a-z])/gu, "$1 $2")
		.replace(/([a-z\d])([A-Z])/gu, "$1 $2")
		.replace(/[_-]+/gu, " ")
		.replace(/\s+/gu, " ")
		.trim();
	return truncateToWidth(readableName, 40, "…");
}

/** Keep the working row calm and private; invocation details remain in the tool card. */
export function formatToolProgress(toolName: string, args: ToolArgs): string {
	const builtInMessage = TOOL_PROGRESS_MESSAGES[toolName];
	if (builtInMessage) return builtInMessage;
	const displayName = humanizeToolName(toolName, args);
	return displayName ? `Using ${displayName}…` : "Running tool…";
}

/** Owns Pi's single working row so animation remains continuous across thinking and tool execution. */
export class ProgressController {
	private context: ExtensionContext | undefined;
	private readonly activeTools = new Map<string, string>();
	private message: string | undefined;
	/** Stops this label's share of the animation clock. */
	private stopTicking: (() => void) | undefined;
	/** The loader frame the border pill shows. */
	private frame = 0;

	private readonly pi: ExtensionAPI;

	constructor(pi: ExtensionAPI) {
		this.pi = pi;
		// Keep only the current phase/active calls while detached. No UI work or
		// clock runs, but re-enabling mid-stream can resume the correct label.
		pi.on("agent_start", () => {
			this.setMessage("Thinking…");
		});
		pi.on("message_update", (event) => {
			const type = event.assistantMessageEvent?.type;
			if (type === "thinking_start" || type === "thinking_delta") this.setMessage("Thinking…");
			else if (type === "text_start" || type === "text_delta") this.setMessage("Responding…");
			else if (type === "toolcall_start" || type === "toolcall_delta") this.setMessage("Preparing tools…");
		});
		pi.on("tool_execution_start", (event) => {
			if (typeof event.toolName !== "string") return;
			const message = formatToolProgress(event.toolName, event.args ?? {});
			this.activeTools.set(event.toolCallId, message);
			this.setMessage(message);
		});
		pi.on("tool_execution_end", (event) => {
			this.activeTools.delete(event.toolCallId);
			const remaining = [...this.activeTools.values()].at(-1);
			this.setMessage(remaining ?? "Processing results…");
		});
		// agent_end closes one low-level run, after which Pi may still retry, compact,
		// or continue from a settlement boundary. Pi hides its own working row at that
		// point and rebuilds it at the next turn_start from the message last set here,
		// so releasing the label at agent_end makes a retried turn reappear under Pi's
		// default label. Release it at the final settlement instead.
		pi.on("agent_settled", () => {
			this.activeTools.clear();
			this.clearMessage();
		});
		// docs/contract.md section 6.9: show the Compacting pill while pi compacts.
		pi.on("session_before_compact", () => { this.setMessage("Compacting…"); });
		const afterCompact = () => {
			if (this.context?.isIdle?.() !== false && this.activeTools.size === 0) this.clearMessage();
			else this.setMessage([...this.activeTools.values()].at(-1) ?? "Thinking…");
		};
		pi.on("session_compact", afterCompact);
		pi.on("session_compact_failed", afterCompact);
	}

	bind(ctx: ExtensionContext, resume = false): void {
		this.stopAnimation();
		hookEditor();
		this.context = ctx;
		const message = resume && ctx.isIdle?.() === false ? this.message ?? "Thinking…" : undefined;
		if (!message) this.activeTools.clear();
		this.message = undefined;
		ctx.ui.setWorkingVisible(true);
		ctx.ui.setWorkingIndicator({ frames: [] });
		if (message) this.setMessage(message);
	}

	dispose(preservePhase = false): void {
		const context = this.context;
		this.context = undefined;
		this.stopAnimation();
		if (!preservePhase) this.activeTools.clear();
		if (!preservePhase) this.message = undefined;
		setWorkingWords(undefined);
		try { context?.ui.setWorkingMessage(); }
		finally { context?.ui.setWorkingIndicator(); }
	}

	private setMessage(message: string): void {
		if (!this.context) { this.message = message; return; }
		if (this.message === message && this.stopTicking) return;
		this.message = message;
		this.renderMessage();
		if (this.stopTicking) return;
		// The label is one fixed color; the host still gets it on each tick of the shared clock.
		this.frame = 0;
		this.stopTicking = onTick(() => {
			this.frame = (this.frame + 1) % LOADER_FRAMES.length;
			this.renderMessage();
		}, LOADER_INTERVAL_MS);
	}

	private renderMessage(): void {
		if (!this.message || !this.context) return;
		setWorkingWords(this.message, this.frame);
		this.context.ui.setWorkingMessage(pill(RUNNING, workingWords(this.message)));
	}

	private clearMessage(): void {
		this.stopAnimation();
		this.message = undefined;
		setWorkingWords(undefined);
		this.context?.ui.setWorkingMessage();
	}

	private stopAnimation(): void {
		this.stopTicking?.();
		this.stopTicking = undefined;
	}
}
