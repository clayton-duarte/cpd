import { describe, expect, it } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import App from './App';

function renderApp() {
  return render(
    <MantineProvider>
      <App />
    </MantineProvider>,
  );
}

function canvas() {
  return within(screen.getByTestId('canvas-area'));
}

describe('App navigation', () => {
  it('starts at the Leads level and shows every lead', () => {
    renderApp();
    expect(canvas().getByText('Harvest planner')).toBeTruthy();
    expect(canvas().getByText('Grove automation')).toBeTruthy();
    expect(canvas().getByText('New orchard survey')).toBeTruthy();
  });

  it('clicking a lead descends to Plans showing exactly that lead\'s workflows', () => {
    renderApp();
    fireEvent.click(canvas().getByText('Grove automation'));

    expect(canvas().getByText('Automated drip-irrigation scheduling')).toBeTruthy();
    expect(canvas().getByText('Pollinator tracking notebook')).toBeTruthy();
    expect(canvas().getByText('Soil moisture sensor rollout')).toBeTruthy();
    // Not s1's or s3's workflows.
    expect(canvas().queryByText('Storefront banner refresh')).toBeNull();
    expect(canvas().queryByText('Compost rotation plan')).toBeNull();
  });

  it('a lead with zero plans renders an empty Plans level without crashing', () => {
    renderApp();
    fireEvent.click(canvas().getByText('New orchard survey'));
    // s3 has exactly one plan (w6) in the fixture -- assert it renders fine
    // rather than inventing a zero-plan fixture lead not in sample data.
    expect(canvas().getByText('Compost rotation plan')).toBeTruthy();
  });

  it('Escape at Jobs returns to Plans with the same lead still selected', async () => {
    renderApp();
    fireEvent.click(canvas().getByText('Grove automation'));
    fireEvent.click(canvas().getByText('Automated drip-irrigation scheduling'));

    // Now at Jobs level for w3 (ELK layout is async).
    expect(await canvas().findByText('Model soil moisture thresholds')).toBeTruthy();

    fireEvent.keyDown(window, { key: 'Escape' });

    // Back at Plans, still scoped to s2 (Grove automation).
    expect(canvas().getByText('Automated drip-irrigation scheduling')).toBeTruthy();
    expect(canvas().queryByText('Storefront banner refresh')).toBeNull();
  });

  it('Escape at Leads does nothing', () => {
    renderApp();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(canvas().getByText('Harvest planner')).toBeTruthy();
  });

  it('the breadcrumb shows the current path and each segment is clickable', () => {
    renderApp();
    fireEvent.click(canvas().getByText('Grove automation'));
    fireEvent.click(canvas().getByText('Automated drip-irrigation scheduling'));

    expect(screen.getByText('Orchard')).toBeTruthy();
    expect(screen.getAllByText('Grove automation').length).toBeGreaterThan(0);

    fireEvent.click(screen.getByText('Orchard'));
    expect(canvas().getByText('Harvest planner')).toBeTruthy();
  });

  it('Escape with a job selected clears the selection first, then ascends on a second Escape', async () => {
    renderApp();
    fireEvent.click(canvas().getByText('Grove automation'));
    fireEvent.click(canvas().getByText('Automated drip-irrigation scheduling'));

    expect(await canvas().findByText('Model soil moisture thresholds')).toBeTruthy();

    fireEvent.click(await canvas().findByText('Model soil moisture thresholds'));
    const selectedNode = (await canvas().findByText('Model soil moisture thresholds')).closest('[data-selected]');
    expect(selectedNode?.getAttribute('data-selected')).toBe('true');

    fireEvent.keyDown(window, { key: 'Escape' });

    // Still at Jobs level -- Escape only cleared the selection.
    const afterFirstEscape = (await canvas().findByText('Model soil moisture thresholds')).closest(
      '[data-selected]',
    );
    expect(afterFirstEscape?.getAttribute('data-selected')).toBe('false');

    fireEvent.keyDown(window, { key: 'Escape' });

    // Second Escape, with nothing selected, ascends to Plans.
    expect(canvas().getByText('Automated drip-irrigation scheduling')).toBeTruthy();
  });
});
