import type { QuestionnaireSessionComponent } from "./state/questionnaire-session.ts";

/**
 * The ask dialog never hangs (docs/commands.md §15.13). An inline mount that is
 * displaced — another dialog took pi's single editor slot, or the TUI stopped
 * rendering it — resolves loud after `RENDER_GUARD_LOST_TICKS` consecutive ticks
 * without a render, and every mount (inline and overlay) settles at
 * `MOUNT_BOUND_MS`. The three durations are pinned by the docs paragraph and the
 * hang-guard tests: never change them.
 */
export const RENDER_GUARD_TICK_MS = 1000;
export const RENDER_GUARD_LOST_TICKS = 3;
export const MOUNT_BOUND_MS = 2 * 60 * 60 * 1000;

/**
 * Render guard for an inline mount. Wraps the mounted component so its render
 * reports back to the guard; a timer ticks every `RENDER_GUARD_TICK_MS` ms,
 * invalidating the mounted component on each tick (so a live dialog keeps
 * proving itself to pi-tui) and counting consecutive ticks without a render.
 * After `RENDER_GUARD_LOST_TICKS` such ticks the slot was taken and the mount is
 * displaced: the guard stops itself and calls the `onDisplaced` callback, which
 * the mount code wires to `done(displaced-result)`.
 *
 * Timer-based and fake-timer friendly: `start`/`stop` own the only interval the
 * guard holds, so the mount's teardown can stop it and leave no timer behind.
 */
export class RenderGuard {
	private readonly inner: QuestionnaireSessionComponent;
	private readonly onDisplaced: () => void;
	private interval: ReturnType<typeof setInterval> | undefined;
	private sawRender = false;
	private missedTicks = 0;

	/** The wrapped component the host mounts and renders; its render reports back to the guard. */
	readonly component: QuestionnaireSessionComponent;

	constructor(inner: QuestionnaireSessionComponent, onDisplaced: () => void) {
		this.inner = inner;
		this.onDisplaced = onDisplaced;
		this.component = {
			render: (width) => {
				this.sawRender = true;
				return this.inner.render(width);
			},
			invalidate: () => this.inner.invalidate(),
			handleInput: (data) => this.inner.handleInput(data),
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
		if (this.sawRender) {
			this.sawRender = false;
			this.missedTicks = 0;
		} else {
			this.missedTicks += 1;
		}
		if (this.missedTicks >= RENDER_GUARD_LOST_TICKS) {
			this.stop();
			this.onDisplaced();
			return;
		}
		// Still live: force the host to re-render the mounted component so a rendered
		// dialog keeps resetting the counter, while a displaced one never renders and
		// trips it on the next ticks.
		this.component.invalidate();
	}
}
