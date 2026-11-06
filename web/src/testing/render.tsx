import type { ReactElement } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom does not implement Element.prototype.getAnimations, a browser always has it. Base UI's
// ScrollAreaViewport calls it from a timer, so define the minimal global only when missing.
if (typeof Element !== "undefined" && typeof Element.prototype.getAnimations !== "function") {
  Element.prototype.getAnimations = (): Animation[] => [];
}

/** Mounts an element into a detached container; unmount is idempotent and removes the container. */
export function mount(element: ReactElement): { container: HTMLElement; unmount(): void } {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => root.render(element));
  let mounted = true;
  return {
    container,
    unmount() {
      if (!mounted) return;
      mounted = false;
      act(() => root.unmount());
      container.remove();
    },
  };
}
