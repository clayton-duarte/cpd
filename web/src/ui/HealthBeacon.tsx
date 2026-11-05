export interface HealthBeaconProps {
  root: () => HTMLElement | null;
  errors: () => number;
  onHealthy: () => void;
}

// Stub: renders nothing.
export function HealthBeacon(_props: HealthBeaconProps): null {
  return null;
}
