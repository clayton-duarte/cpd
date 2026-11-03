import type { ActionOutput, CockpitAction } from "../../protocol/cockpit.ts";
import type { Cpd } from "./cli.ts";

export function actionArgs(_action: CockpitAction): readonly string[] {
  throw new Error("not implemented");
}
export function runAction(_cpd: Cpd, _raw: unknown): Promise<ActionOutput> {
  throw new Error("not implemented");
}
export const WEB_UNBLOCKS = "User message from the web chat";
export function sessionSendArgs(_sessionId: string, _text: string, _unblocks?: string): readonly string[] {
  throw new Error("not implemented");
}
export function sessionRetierArgs(_sessionId: string, _tier: string): readonly string[] {
  throw new Error("not implemented");
}
