import { useEffect, useRef } from "react";

export interface HealthBeaconProps {
  root: () => HTMLElement | null;
  errors: () => number;
  onHealthy: () => void;
}

export function HealthBeacon(props: HealthBeaconProps): null {
  const latest = useRef(props);
  latest.current = props;
  useEffect(() => {
    const { root, errors, onHealthy } = latest.current;
    const el = root();
    if (el !== null && el.childElementCount > 0 && errors() === 0) onHealthy();
  }, []);
  return null;
}
