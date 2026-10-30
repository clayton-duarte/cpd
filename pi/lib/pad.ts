import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import type { Component } from "@earendil-works/pi-tui";
import { Container, HStack, Spacer, TuiAltScreen, truncateToWidth, VStack, visibleWidth } from "@earendil-works/pi-tui";
import { getLayoutNode, LAYOUT_NODE } from "@earendil-works/pi-tui/dist/layout-node.js";
import { dispatchMouseEvent, type TuiMouseEvent } from "@earendil-works/pi-tui/dist/tui.js";
import { hookMethod } from "../compact-tools/extensions/compact-tools-hook.ts";

/**
 * The uniform 1-cell chat-slot pad (docs/research/chat-padding.md): the pi pane's fullscreen
 * layout root is wrapped so it renders with one blank row above, one blank row below, one blank column left and one
 * right, and the real root grows in the middle. pi 1.3.0 has no padding option on the fullscreen root, so the
 * fullscreen renderer's own `setLayoutRoot` is wrapped (as `compact-tools-viewport.ts` wraps `doRender`) and reaches
 * pi's TUI through the `cpd-pad` widget. The wrapper is installed once per process, before pi's first mount, so the
 * first root is wrapped too; every later mount goes through the same hook.
 */

const WIDGET_KEY = "cpd-pad";

/** One blank row: a `Spacer` measured at exactly one cell. */
const padRow = (): { component: Component; basis: number; minSize: number; maxSize: number } =>
  ({ component: new Spacer(1), basis: 1, minSize: 1, maxSize: 1 });

/**
 * What the `HStack` holds in place of a root that has a layout node. pi-tui renders an `HStack` child once to measure
 * its height and lays it out again (rendering the whole transcript a second time); the height is unused here (a
 * `basis: 0` stretched `HStack`), so this renders nothing, and `layoutComponent` walks the real root's layout node:
 * the real boxes (the `ScrollView`, the dock, the editor) are laid out and rendered once. Mouse events that the
 * real root handles itself are forwarded to it; a root with the default container `handleMouse` is skipped by
 * pi-tui, as this stand-in is.
 */
class RootStandIn extends Container {
  private readonly root: Component;

  constructor(root: Component) {
    super();
    this.root = root;
    if (root.handleMouse !== Container.prototype.handleMouse) {
      this.handleMouse = (event: TuiMouseEvent) => dispatchMouseEvent(root, event);
    }
  }

  override render(): string[] {
    return [];
  }

  override invalidate(): void {
    this.root.invalidate?.();
  }

  [LAYOUT_NODE]() {
    return getLayoutNode(this.root);
  }
}

/**
 * The layout root with one blank cell on each side. It is a `VStack`/`HStack` so the real root stays a laid-out child
 * (the transcript's `ScrollView` keeps its rect) and grows into the middle; `render` is the same inset for a direct
 * call, with no compositor resets around the side columns.
 */
class PaddedRoot extends VStack {
  private readonly inner: Component;

  constructor(root: Component) {
    super([
      padRow(),
      {
        component: new HStack([
          padRow(),
          { component: getLayoutNode(root) ? new RootStandIn(root) : root, basis: 0, grow: 1, shrink: 1, minSize: 1 },
          padRow(),
        ]),
        basis: 0,
        grow: 1,
        shrink: 1,
        minSize: 1,
      },
      padRow(),
    ]);
    this.inner = root;
  }

  override render(width: number): string[] {
    const safeWidth = Math.max(1, Math.floor(width));
    const blank = " ".repeat(safeWidth);
    const innerWidth = Math.max(1, safeWidth - 2);
    const lines = this.inner.render(innerWidth).map((line) => {
      const body = ` ${line}`;
      const padded = body + " ".repeat(Math.max(0, safeWidth - visibleWidth(body)));
      return visibleWidth(padded) > safeWidth ? truncateToWidth(padded, safeWidth, "") : padded;
    });
    return [blank, ...lines, blank];
  }
}

/** The fullscreen layout root with the 1-cell pad; `undefined` is passed through unchanged. */
export function paddedRoot(root: Component): Component {
  return new PaddedRoot(root);
}

/**
 * Wrap `TuiAltScreen.prototype.setLayoutRoot` once per process, so every fullscreen mount (a `/reload` or a
 * `/settings` mode switch builds a fresh TUI) insets its root.
 */
export function registerChatPad(): void {
  const prototype = (TuiAltScreen as unknown as { prototype?: object } | undefined)?.prototype;
  if (!prototype) return;
  hookMethod(prototype, "setLayoutRoot", "cpd-pad.setLayoutRoot", (self, args, original) => {
    const root = args[0] as Component | undefined;
    return original.apply(self, [root === undefined ? undefined : paddedRoot(root)]);
  });
}

/** Install the pad: hook the fullscreen root once, then the invisible widget that is pi's way to reach the TUI. */
export function installChatPad(ctx: ExtensionContext): void {
  registerChatPad();
  ctx.ui.setWidget(WIDGET_KEY, () => ({ render: () => [], invalidate() {} }), { placement: "belowEditor" });
}
