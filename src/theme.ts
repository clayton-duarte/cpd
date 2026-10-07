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
});
