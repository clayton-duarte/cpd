import { ChevronDownIcon } from "lucide-react";
import { useRef } from "react";
import type {
  Capabilities,
  ConfigureChange,
  FooterStats,
  ModelChoice,
  Stat,
} from "../../protocol/domain.ts";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "../components/dropdown-menu.tsx";
import { InputGroupButton } from "../components/input-group.tsx";

export type ConfigureHandler = (change: ConfigureChange) => void;

const BUSY_REASON =
  "A run is pending: the model changes when the session is idle";
const known = <T,>(s: Stat<T> | undefined): T | null =>
  s !== undefined && s.state === "known" ? s.value : null;
const withCurrent = (
  list: readonly string[],
  current: string | null,
): string[] =>
  current !== null && !list.includes(current)
    ? [...list, current]
    : [...list];

export function TierSelector(props: {
  capabilities: Capabilities | null;
  footer: FooterStats | null;
  models: readonly ModelChoice[] | null;
  busy: boolean;
  onConfigure: ConfigureHandler;
  onRequestModels(): void;
}): React.JSX.Element {
  const { capabilities: caps, footer, models, busy } = props;
  const requested = useRef(false);
  const tier = known(footer?.tier);
  const model = known(footer?.model);
  const thinking = known(footer?.thinking);

  const reasonOf = (
    s: { supported: boolean; reason?: string } | undefined,
    busyBlocks: boolean,
  ): string | null => {
    if (caps === null || s === undefined) return "Not attached";
    if (!s.supported) return s.reason ?? "Unsupported";
    return busyBlocks && busy ? BUSY_REASON : null;
  };
  const tierReason = reasonOf(caps?.configure.tier, false);
  const modelReason = reasonOf(caps?.configure.model, true);
  const thinkingReason = reasonOf(caps?.configure.thinking, true);

  const tiers = withCurrent(caps?.configure.tier.options ?? [], tier);
  const modelOptions =
    models !== null && models.length > 0
      ? models.map((m) => ({ value: m.model, text: m.name }))
      : model !== null
        ? [{ value: model, text: model.slice(model.lastIndexOf("/") + 1) }]
        : [];
  const levels = withCurrent(
    models?.find((m) => m.model === model)?.thinkingLevels ?? [],
    thinking,
  );

  const shortModel =
    model === null
      ? "Model"
      : (modelOptions.find((o) => o.value === model)?.text ??
        model.slice(model.lastIndexOf("/") + 1));

  return (
    <div
      data-part="tier-selector"
      className="flex flex-wrap items-center gap-1"
    >
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <InputGroupButton
            size="sm"
            aria-label="Tier"
            disabled={tierReason !== null || tiers.length === 0}
            title={tierReason ?? undefined}
            className="font-mono text-xs"
          >
            {tier ?? "Tier"}
            <ChevronDownIcon />
          </InputGroupButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuRadioGroup
            value={tier ?? ""}
            onValueChange={(val) =>
              props.onConfigure({ kind: "tier", tier: val })
            }
          >
            <DropdownMenuGroup>
              <DropdownMenuLabel>Tier</DropdownMenuLabel>
              {tiers.map((t) => (
                <DropdownMenuRadioItem
                  key={t}
                  value={t}
                  className="font-mono text-xs"
                >
                  {t}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuGroup>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu
        onOpenChange={(open) => {
          if (open && !requested.current) {
            requested.current = true;
            props.onRequestModels();
          }
        }}
      >
        <DropdownMenuTrigger asChild>
          <InputGroupButton
            size="sm"
            aria-label="Model"
            disabled={modelReason !== null}
            title={modelReason ?? undefined}
            className="font-mono text-xs"
          >
            {shortModel}
            <ChevronDownIcon />
          </InputGroupButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuRadioGroup
            value={model ?? ""}
            onValueChange={(val) =>
              props.onConfigure({ kind: "model", model: val })
            }
          >
            <DropdownMenuGroup>
              <DropdownMenuLabel>Model</DropdownMenuLabel>
              {modelOptions.map((o) => (
                <DropdownMenuRadioItem
                  key={o.value}
                  value={o.value}
                  className="font-mono text-xs"
                >
                  {o.text}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuGroup>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <InputGroupButton
            size="sm"
            aria-label="Thinking level"
            disabled={thinkingReason !== null || levels.length === 0}
            title={thinkingReason ?? undefined}
            className="font-mono text-xs"
          >
            {thinking ?? "Effort"}
            <ChevronDownIcon />
          </InputGroupButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuRadioGroup
            value={thinking ?? ""}
            onValueChange={(val) =>
              props.onConfigure({ kind: "thinking", level: val })
            }
          >
            <DropdownMenuGroup>
              <DropdownMenuLabel>Effort</DropdownMenuLabel>
              {levels.map((l) => (
                <DropdownMenuRadioItem
                  key={l}
                  value={l}
                  className="font-mono text-xs"
                >
                  {l}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuGroup>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
