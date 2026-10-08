import { Group } from '@mantine/core';
import { PlanCard } from '../components/PlanCard';
import type { PlanCardData } from '../model/levels';
import { SPACE_CARDS } from '../layout/spacing';

export interface PlansLevelProps {
  plans: PlanCardData[];
  onSelect: (planId: string) => void;
}

/**
 * Plans level: one card per workflow of the selected lead. Plans have no
 * dependency edges between them -- a plain wrapping flow is correct, derived
 * from CSS layout rather than any hand-written position (D26).
 */
export function PlansLevel({ plans, onSelect }: PlansLevelProps) {
  return (
    <Group gap={`${SPACE_CARDS}px`} align="flex-start" p={`${SPACE_CARDS}px`} wrap="wrap">
      {plans.map((plan) => (
        <PlanCard key={plan.id} plan={plan} onClick={() => onSelect(plan.id)} />
      ))}
    </Group>
  );
}
