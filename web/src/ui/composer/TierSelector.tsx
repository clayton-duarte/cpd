import type { Capabilities, ConfigureChange, FooterStats, ModelChoice } from "../../protocol/domain.ts";

export type ConfigureHandler = (change: ConfigureChange) => void;

export function TierSelector(_props: { capabilities: Capabilities | null; footer: FooterStats | null; models: readonly ModelChoice[] | null; busy: boolean; onConfigure: ConfigureHandler; onRequestModels(): void }): React.JSX.Element {
  throw new Error("not implemented");
}
