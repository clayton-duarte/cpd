import { useEffect, useRef } from "react";

export interface HealthBeaconProps {
  /** Whether the first `cockpit.snapshot` has arrived: a painted shell with no data is not health. */
  live: boolean;
  /** The element the shell painted into: an empty root is not health. */
  root: () => HTMLElement | null;
  /** The crash-signal count: a counted error is not health. */
  errors: () => number;
  onHealthy: () => void;
}

/**
 * Reports `health:ok` at most once, and only after the first `cockpit.snapshot` (web/CONTRACT.md
 * §15.2): with the store live, a non-empty root and no counted errors. The earlier mount-time report
 * said nothing about data, so a boot whose stream was dead still recorded healthy. The effect runs
 * on every render (there is no dependency to miss) and a ref keeps the report to one.
 */
export function HealthBeacon(props: HealthBeaconProps): null {
  const reported = useRef(false);
  useEffect(() => {
    if (reported.current) return;
    if (!props.live) return;
    const el = props.root();
    if (el === null || el.childElementCount === 0 || props.errors() !== 0) return;
    reported.current = true;
    props.onHealthy();
  });
  return null;
}
