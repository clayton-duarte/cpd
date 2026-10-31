import type { TUI } from "@earendil-works/pi-tui";
import type { QuestionnaireSessionComponent } from "./state/questionnaire-session.ts";

/**
 * The ask dialog never hangs (docs/commands.md §15.13). An inline mount that is
 * displaced — another dialog took pi's single editor slot and cleared the editor
 * container — resolves loud as soon as pi-tui reports the mounted component has
 * left that container; otherwise the mount keeps asking pi-tui for a frame so a
 * live question keeps being drawn. Every mount (inline and overlay) settles at
 * `MOUNT_BOUND_MS`. The three durations are pinned by the docs paragraph and the
 * hang-guard tests: never change them.
 *
 * `RENDER_GUARD_LOST_TICKS` no longer decides anything: the old guard counted
 * consecutive ticks without a render, but pi-tui is demand-driven and a missing
 * frame does not mean displacement. It is kept because the contract pins the
 * duration.
 */
export const RENDER_GUARD_TICK_MS = 1000;
export const RENDER_GUARD_LOST_TICKS = 3;
export const MOUNT_BOUND_MS = 2 * 60 * 60 * 1000;

/**
 * `Tui.isComponentMounted` is the mounted check pi-tui exposes on its TUI
 * implementations, but it is absent from the public `TUI` type (private on the
 * base class). The guard reaches it through this narrow view.
 */
type MountedCheckTui = TUI & {
	isComponentMounted(component: QuestionnaireSessionComponent): boolean;
};

/**
 * Render guard for an inline mount. Holds the `Tui` it was constructed with and a
 * timer ticking every `RENDER_GUARD_TICK_MS` ms. On each tick it asks
 * `tui.isComponentMounted(this.component)` — the component the factory returns and
 * pi mounted — and:
 *
 * - while mounted, it calls `tui.requestRender()` so a mounted question keeps being
 *   drawn, and never resolves;
 * - when not mounted, the inline slot was taken (another dialog cleared the editor
 *   container): the guard stops itself and calls `onDisplaced`, which the mount
 *   wires to `done(displaced-result)`.
 *
 * Timer-based and fake-timer friendly: `start`/`stop` own the only interval the
 * guard holds, so the mount's teardown can stop it and leave no timer behind.
 */
export class RenderGuard {
	private readonly tui: MountedCheckTui;
	private readonly onDisplaced: () => void;
	private interval: ReturnType<typeof setInterval> | undefined;

	/** The wrapped component the host mounts and renders; `isComponentMounted` is asked about it. */
	readonly component: QuestionnaireSessionComponent;

	constructor(tui: TUI, inner: QuestionnaireSessionComponent, onDisplaced: () => void) {
		this.tui = tui as MountedCheckTui;
		this.onDisplaced = onDisplaced;
		this.component = {
			render: (width) => inner.render(width),
			invalidate: () => inner.invalidate(),
			handleInput: (data) => inner.handleInput(data),
		};
	}

	/** Start ticking every `interval` ms. Idempotent: a started guard does not restart. */
	start(interval: number): void {
		if (this.interval !== undefined) return;
		this.interval = setInterval(() => this.tick(), interval);
	}

	/** Stop ticking. Idempotent; call on settle, on displacement, and on teardown. */
	stop(): void {
		if (this.interval === undefined) return;
		clearInterval(this.interval);
		this.interval = undefined;
	}

	private tick(): void {
		if (!this.tui.isComponentMounted(this.component)) {
			this.stop();
			this.onDisplaced();
			return;
		}
		// Still mounted: ask pi-tui for a frame so the live question keeps being drawn.
		this.tui.requestRender();
	}
}
