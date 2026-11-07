export interface HealthBeaconProps {
  root: () => HTMLElement | null;
  errors: () => number;
  onHealthy: () => void;
}

/**
 * The desktop no longer reports health at mount (web/CONTRACT.md §15.2): `health:ok` waits for the
 * first `cockpit.snapshot`, which the App reports itself (web/src/ui/App.tsx, `onHealthy`). This
 * component keeps its props for the shell's existing wiring and never reports.
 */
export function HealthBeacon(_props: HealthBeaconProps): null {
  return null;
}
