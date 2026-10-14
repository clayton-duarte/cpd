/** Contract §15: the adapter is on inside a herdr pane (`HERDR_ENV=1`) or a tmux pane (`TMUX_PANE`). */
export function adapterActive(env: Record<string, string | undefined>): boolean {
  return env.HERDR_ENV === "1" || Boolean(env.TMUX_PANE);
}
