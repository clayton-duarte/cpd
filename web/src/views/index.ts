// The cockpit views barrel (docs/research/web-client-api.md §6): Tree, Plan, Inbox, Logs
// and Chat, all read-only (A7), plus each view's prop type.
export { Tree, type TreeProps } from "./tree";
export { Plan, type PlanProps } from "./plan";
export { Inbox, type InboxProps } from "./inbox";
export { Logs, type LogEntry, type LogsProps } from "./logs";
export { Chat, type ChatFrame, type ChatProps } from "./chat";
