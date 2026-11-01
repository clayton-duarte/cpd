// The Chat's authored follow-the-live-edge rule (web/README.md "The thread follows its live
// edge", hs-d832). The vendored MessageScroller stays the layout shell with `autoScroll` off:
// where the viewport sits in the thread is this rule's, not the primitive's 8px edge. Following
// starts true; a scroll more than 40% of the viewport height above the newest content releases
// it, and while following every rendered frame pins the view to the bottom. A released view is
// left exactly where the reader put it, and the jump-to-latest control re-engages following.
//
// The hook owns the viewport's `data-autoscrolling` attribute, the flag the themed scrollbar
// reads to hide the thumb while the thread follows: the primitive would clear it on its own
// scroll bookkeeping, so the authored rule re-asserts it.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";

/** How much of the viewport above the newest content may be scrolled before following releases. */
const FOLLOW_MARGIN = 0.4;

/** The scroll position the thread pins to: the bottom of its content. */
function bottom(element: HTMLElement): number {
  return Math.max(0, element.scrollHeight - element.clientHeight);
}

/** The authored rule's surface, as `LiveChat` uses it. */
export interface ThreadScroll {
  /** Attach to the scrolling viewport (`[data-slot="message-scroller-viewport"]`). */
  viewportRef: (element: HTMLElement | null) => void;
  /** Whether the view is following the newest content. */
  following: boolean;
  /** Re-engage following and pin the view to the newest content. */
  jumpToLatest: () => void;
}

/**
 * The authored follow rule. `content` is the rendered messages: a new value is a frame, and while
 * following the layout effect pins the viewport to the bottom.
 */
export function useThreadScroll(content: unknown): ThreadScroll {
  const elementRef = useRef<HTMLElement | null>(null);
  const followingRef = useRef(true);
  const [following, setFollowing] = useState(true);
  const [element, setElement] = useState<HTMLElement | null>(null);

  // A callback ref, so the scroll listener attaches when the viewport mounts (and again if it
  // ever remounts), never only on the first render.
  const viewportRef = useCallback((node: HTMLElement | null) => {
    elementRef.current = node;
    setElement(node);
  }, []);

  useEffect(() => {
    if (element === null) return;
    const onScroll = (): void => {
      const gap = element.scrollHeight - element.scrollTop - element.clientHeight;
      const next = gap <= FOLLOW_MARGIN * element.clientHeight;
      // The primitive clears `data-autoscrolling` on every scroll of its own; re-assert the
      // authored flag after it, whether or not the following state itself changed.
      element.toggleAttribute("data-autoscrolling", next);
      if (next === followingRef.current) return;
      followingRef.current = next;
      // The control's appearance is the reader's feedback, so it lands with the scroll event.
      flushSync(() => setFollowing(next));
    };
    element.addEventListener("scroll", onScroll);
    return () => element.removeEventListener("scroll", onScroll);
  }, [element]);

  useLayoutEffect(() => {
    const node = elementRef.current;
    if (node === null || !followingRef.current) return;
    node.scrollTop = bottom(node);
    node.toggleAttribute("data-autoscrolling", true);
    // The primitive's content observer commits on the same microtask boundary and clears the
    // flag; re-assert it once that commit has run.
    queueMicrotask(() => {
      if (elementRef.current === node && followingRef.current) {
        node.toggleAttribute("data-autoscrolling", true);
      }
    });
  }, [content]);

  const jumpToLatest = useCallback(() => {
    const node = elementRef.current;
    followingRef.current = true;
    setFollowing(true);
    if (node === null) return;
    node.scrollTop = bottom(node);
    node.toggleAttribute("data-autoscrolling", true);
  }, []);

  return { viewportRef, following, jumpToLatest };
}
