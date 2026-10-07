import { createTheme, colorsTuple } from '@mantine/core';

export const theme = createTheme({
  primaryColor: 'blue',
  primaryShade: 5,
  colors: {
    blue: colorsTuple('var(--blue)'),
    red: colorsTuple('var(--red)'),
    green: colorsTuple('var(--green)'),
    dark: [
      'var(--fg-bright)', 'var(--fg-bright)', 'var(--fg-muted)', 'var(--fg-faint)',
      'var(--line-strong)', 'var(--line)', 'var(--bg-panel)', 'var(--bg-deep)', 'var(--bg-deep)', 'var(--bg-deep)',
    ],
  },
  fontFamily: 'Inter, system-ui, -apple-system, sans-serif',
  defaultRadius: 'sm',
  // Mantine's named spacing/radius scales are wired to the single spacing
  // scale defined in theme.css (B4/D78) so "xs"/"sm"/etc never diverge from
  // --space-N -- components reach for Mantine's props, never a raw px.
  spacing: {
    xs: 'var(--space-2)',
    sm: 'var(--space-3)',
    md: 'var(--space-4)',
    lg: 'var(--space-5)',
    xl: 'var(--space-6)',
  },
  radius: {
    xs: 'var(--space-1)',
    sm: 'var(--space-1)',
    md: 'var(--space-2)',
    lg: 'var(--space-3)',
    xl: 'var(--space-4)',
  },
});
