/** The Event cursor from the stored entries (contract §6.13 "Delivery (P1.4)", 2): the greater of `registered_event` and the highest delivered id of this Session. */
export function cursorFrom(_registeredEvent: number, _entries: readonly unknown[], _session: string): number {
  throw new Error("not implemented");
}
