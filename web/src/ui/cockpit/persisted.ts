/** Boolean state persisted in localStorage under `cpd-ui:collapse:<key>` ('1' open, '0' closed). Stub. */
export function usePersistedBoolean(_key: string, defaultValue: boolean): [boolean, (next: boolean) => void] {
  return [defaultValue, () => {}];
}
