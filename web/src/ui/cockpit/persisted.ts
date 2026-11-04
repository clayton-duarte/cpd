import { useCallback, useState } from "react";

/** Boolean state persisted in localStorage under `cpd-ui:collapse:<key>` ('1' true, '0' false). */
export function usePersistedBoolean(
  key: string,
  initial: boolean,
): [boolean, (next: boolean) => void] {
  const storageKey = `cpd-ui:collapse:${key}`;
  const [value, setValue] = useState<boolean>(() => {
    try {
      const raw = window.localStorage.getItem(storageKey);
      if (raw === "1") return true;
      if (raw === "0") return false;
    } catch {
      // storage unavailable: fall back to the default
    }
    return initial;
  });
  const update = useCallback(
    (next: boolean) => {
      setValue(next);
      try {
        window.localStorage.setItem(storageKey, next ? "1" : "0");
      } catch {
        // storage unavailable: keep in-memory state only
      }
    },
    [storageKey],
  );
  return [value, update];
}
