// The WebSocket liveness check the cockpit-health verdict uses (air-d88): the server heartbeat
// (`pong`, web/CONTRACT.md §15.2) is the transport's pulse, so a heartbeat that stopped arriving
// counts as a stalled socket. No heartbeat was ever seen means there is nothing to measure.

/** Whether the last heartbeat is older than `timeoutMs`. `undefined` is never stalled. */
export function isStalled(heartbeatAt: number | undefined, now: number, timeoutMs: number): boolean {
  return heartbeatAt !== undefined && now - heartbeatAt >= timeoutMs;
}
