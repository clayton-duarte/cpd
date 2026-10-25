import type { Component } from "@earendil-works/pi-tui";

/**
 * The uniform 1-cell chat-slot pad (decision hs-d684 item 3, docs/research/chat-padding.md): wrap the pi pane's
 * fullscreen layout root so it renders with one blank row above, one blank row below, one blank column left and one
 * right, with `root` in the middle. Stub: returns the root unchanged, so the red tests fail on the inset assertion.
 */
export function paddedRoot(root: Component): Component {
  return root;
}
