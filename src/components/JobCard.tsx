import { Badge, Group, Text, Tooltip } from '@mantine/core';
import { CardShell } from './CardShell';
import {
  IconBan,
  IconCheck,
  IconChecks,
  IconCircleDashed,
  IconClock,
  IconHandStop,
  IconLoader2,
  IconPaperclip,
  IconPlayerPause,
  IconSteeringWheel,
  IconX,
} from '@tabler/icons-react';
import type { Job } from '../model/types';
import { isDashed, isDimmed, jobColor, showAttempt, statusIcon, tierLabel } from '../model/derive';

const ICON_COMPONENTS: Record<string, typeof IconCheck> = {
  'circle-dashed': IconCircleDashed,
  'loader-2': IconLoader2,
  'hand-stop': IconHandStop,
  x: IconX,
  clock: IconClock,
  'player-pause': IconPlayerPause,
  checks: IconChecks,
  check: IconCheck,
  ban: IconBan,
};

const COLOR_VALUES: Record<ReturnType<typeof jobColor>, string> = {
  red: 'var(--red)',
  green: 'var(--green)',
  blue: 'var(--blue)',
  white: 'var(--fg-bright)',
};

const EDGE_VALUES: Record<ReturnType<typeof jobColor>, string> = {
  red: 'var(--red-edge)',
  green: 'var(--green-edge)',
  blue: 'var(--blue-edge)',
  white: 'var(--line-strong)',
};

export interface JobCardProps {
  job: Job;
  /** When true, all borders and icons render as white — "Disable color" toggle. */
  colorDisabled?: boolean;
}

export function JobCard({ job, colorDisabled = false }: JobCardProps) {
  const color = colorDisabled ? 'white' : jobColor(job);
  const colorValue = COLOR_VALUES[color];
  const borderColor = EDGE_VALUES[color];
  const Icon = ICON_COMPONENTS[statusIcon(job.status)];

  return (
    <CardShell borderColor={borderColor} dashed={isDashed(job)} opacity={isDimmed(job) ? 0.45 : 1}>
      <Group justify="space-between" wrap="nowrap" gap="var(--gap)">
        <Group gap="var(--space-1)" wrap="nowrap" style={{ minWidth: 0 }}>
          <Icon size={16} color={colorValue} />
          <Tooltip label={job.title}>
            <Text size="sm" truncate style={{ minWidth: 0 }}>
              {job.title}
            </Text>
          </Tooltip>
        </Group>
        <Group gap="var(--space-1)" wrap="nowrap" style={{ flexShrink: 0 }}>
          {job.steeringPending && <IconSteeringWheel size={16} />}
          {job.artifactCount > 0 && (
            <Group gap="var(--space-1)" wrap="nowrap">
              <IconPaperclip size={14} />
              <Text size="xs" c="var(--fg-muted)">
                {job.artifactCount}
              </Text>
            </Group>
          )}
        </Group>
      </Group>
      <Group justify="space-between" align="center" mt="var(--gap)">
        <Text size="xs" c="var(--fg-faint)">
          {showAttempt(job) ? `#${job.attempt}` : ''}
        </Text>
        <Badge color="dark" variant="light" c={tierLabel(job) === '··' ? 'var(--fg-faint)' : 'var(--fg-bright)'}>
          {tierLabel(job)}
        </Badge>
      </Group>
    </CardShell>
  );
}
