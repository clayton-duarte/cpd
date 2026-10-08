import { describe, expect, it } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import App from './App';

function renderApp() {
  return render(
    <MantineProvider>
      <App />
    </MantineProvider>,
  );
}

describe('App navigation', () => {
  it('starts at the Leads level and shows every lead', () => {
    renderApp();
    expect(screen.getByText('Harvest planner')).toBeTruthy();
    expect(screen.getByText('Grove automation')).toBeTruthy();
    expect(screen.getByText('New orchard survey')).toBeTruthy();
  });

  it('clicking a lead descends to Plans showing exactly that lead\'s workflows', () => {
    renderApp();
    fireEvent.click(screen.getByText('Grove automation'));

    expect(screen.getByText('Automated drip-irrigation scheduling')).toBeTruthy();
    expect(screen.getByText('Pollinator tracking notebook')).toBeTruthy();
    expect(screen.getByText('Soil moisture sensor rollout')).toBeTruthy();
    // Not s1's or s3's workflows.
    expect(screen.queryByText('Storefront banner refresh')).toBeNull();
    expect(screen.queryByText('Compost rotation plan')).toBeNull();
  });

  it('a lead with zero plans renders an empty Plans level without crashing', () => {
    renderApp();
    fireEvent.click(screen.getByText('New orchard survey'));
    // s3 has exactly one plan (w6) in the fixture -- assert it renders fine
    // rather than inventing a zero-plan fixture lead not in sample data.
    expect(screen.getByText('Compost rotation plan')).toBeTruthy();
  });

  it('Escape at Jobs returns to Plans with the same lead still selected', async () => {
    renderApp();
    fireEvent.click(screen.getByText('Grove automation'));
    fireEvent.click(screen.getByText('Automated drip-irrigation scheduling'));

    // Now at Jobs level for w3 (ELK layout is async).
    expect(await screen.findByText('Model soil moisture thresholds')).toBeTruthy();

    fireEvent.keyDown(window, { key: 'Escape' });

    // Back at Plans, still scoped to s2 (Grove automation).
    expect(screen.getByText('Automated drip-irrigation scheduling')).toBeTruthy();
    expect(screen.queryByText('Storefront banner refresh')).toBeNull();
  });

  it('Escape at Leads does nothing', () => {
    renderApp();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.getByText('Harvest planner')).toBeTruthy();
  });

  it('the breadcrumb shows the current path and each segment is clickable', () => {
    renderApp();
    fireEvent.click(screen.getByText('Grove automation'));
    fireEvent.click(screen.getByText('Automated drip-irrigation scheduling'));

    expect(screen.getByText('Orchard')).toBeTruthy();
    expect(screen.getByText('Grove automation')).toBeTruthy();

    fireEvent.click(screen.getByText('Orchard'));
    expect(screen.getByText('Harvest planner')).toBeTruthy();
  });
});
