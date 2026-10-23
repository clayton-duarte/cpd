import { ToolExecutionComponent, type ToolDefinition } from "@earendil-works/pi-coding-agent";
import { hookMethod, type MethodBehavior } from "./compact-tools-hook.ts";
import { refreshRowShell } from "./compact-tools-rendering.ts";

// Symbol.for keys are shared by every evaluation of this module. The prototype is
// Pi's and outlives a /reload, so the patch is installed once and always asks the
// most recently loaded extension instance what to render: the newest load wins.
const RESOLVER_KEY = Symbol.for("pi-compact-tools.custom.resolver.v2");
const LEGACY_RESOLVER_KEY = Symbol.for("pi-compact-tools.custom.resolver");
const STATE_KEY = Symbol.for("pi-compact-tools.custom.state.v2");
const ROUTED_METHODS = ["getCallRenderer", "getResultRenderer", "getRenderShell", "hasRendererDefinition"] as const;

type AnyToolDefinition = ToolDefinition<any, any, any>;

/** The fields of Pi's tool row that decide which renderer is used. */
export interface ToolRow {
	toolName: string;
	toolDefinition?: AnyToolDefinition;
}

export interface RowRenderers {
	renderCall: NonNullable<AnyToolDefinition["renderCall"]>;
	renderResult: NonNullable<AnyToolDefinition["renderResult"]>;
}

export type RowResolver = (row: ToolRow) => RowRenderers | undefined;

type RoutedPrototype = Record<(typeof ROUTED_METHODS)[number], (...args: unknown[]) => unknown>;
type RowState = {
	decisions: WeakMap<object, { resolver: RowResolver | undefined; renderers: RowRenderers | null }>;
	owners: WeakMap<object, { call?: unknown; result?: unknown }>;
};
type ResolverSlot = typeof globalThis & { [RESOLVER_KEY]?: RowResolver };

export function setRowResolver(resolver: RowResolver | undefined): void {
	// The v1 patch may survive an npm update plus /reload. Keep its slot empty.
	(globalThis as unknown as Record<symbol, unknown>)[LEGACY_RESOLVER_KEY] = undefined;
	(globalThis as ResolverSlot)[RESOLVER_KEY] = resolver;
}

/** Remove this instance's resolver when the UI extension is unloaded. */
export function clearRowResolver(resolver: RowResolver): void {
	if ((globalThis as ResolverSlot)[RESOLVER_KEY] === resolver) setRowResolver(undefined);
}

/**
 * Pi routes every renderer lookup on a tool row through these four methods, so
 * answering them is enough to give any tool, whichever extension registered it,
 * compact renderers. Returns false when the host no longer has that shape.
 */
export function patchToolRows(prototype: object | undefined): boolean {
	if (!prototype) return false;
	const routed = prototype as RoutedPrototype;
	if (!ROUTED_METHODS.every((method) => typeof routed[method] === "function")) return false;
	hookMethod(prototype, "updateDisplay", "custom.refreshShell", (self, args, original) => {
		refreshRowShell(self);
		return original.apply(self, args);
	});
	// Cache within a configuration generation. session_start can apply trusted
	// project settings after rows were constructed; a new resolver invalidates
	// those provisional decisions, and updateDisplay reconciles the shell.
	// That state outlives a load, so it lives in a process-global slot shared by all loads.
	const slot = globalThis as unknown as Record<symbol, RowState | undefined>;
	const { decisions, owners } = (slot[STATE_KEY] ??= { decisions: new WeakMap(), owners: new WeakMap() });
	const changed = (row: object, kind: "call" | "result", renderer: unknown) => {
		let owner = owners.get(row);
		if (!owner) owners.set(row, owner = {});
		if (owner[kind] !== renderer) {
			(row as Record<string, unknown>)[kind === "call" ? "callRendererComponent" : "resultRendererComponent"] = undefined;
			owner[kind] = renderer;
		}
		return renderer;
	};
	const resolve = (row: ToolRow): RowRenderers | undefined => {
		const resolver = (globalThis as ResolverSlot)[RESOLVER_KEY];
		let decision = decisions.get(row);
		if (!decision || decision.resolver !== resolver) {
			let renderers: RowRenderers | null;
			try { renderers = resolver?.(row) ?? null; } catch { renderers = null; }
			decision = { resolver, renderers };
			decisions.set(row, decision);
		}
		return decision.renderers ?? undefined;
	};
	// Every call re-hooks, so each record holds this load's behaviour. A plain wrapper from
	// an older release (no hook record, no handle to its original) is not unwrapped: the
	// hook goes on top and the older layer stays as an inert extra call.
	type Original = (...args: unknown[]) => unknown;
	const onRow = (body: (this: ToolRow, original: Original, args: unknown[]) => unknown): MethodBehavior =>
		(self, args, original) => body.call(self, original, args);
	hookMethod(prototype, "getCallRenderer", "custom.getCallRenderer", onRow(function (original, args) {
		return changed(this, "call", resolve(this)?.renderCall ?? original.apply(this, args));
	}));
	hookMethod(prototype, "getResultRenderer", "custom.getResultRenderer", onRow(function (original, args) {
		return changed(this, "result", resolve(this)?.renderResult ?? original.apply(this, args));
	}));
	hookMethod(prototype, "getRenderShell", "custom.getRenderShell", onRow(function (original, args) {
		return resolve(this) ? "self" : original.apply(this, args);
	}));
	hookMethod(prototype, "hasRendererDefinition", "custom.hasRendererDefinition", onRow(function (original, args) {
		return resolve(this) ? true : original.apply(this, args);
	}));
	return true;
}

export function installToolRowPatch(): boolean {
	return patchToolRows(ToolExecutionComponent?.prototype);
}
