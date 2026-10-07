import { createTheme, colorsTuple } from '@mantine/core';

export const TOKENS = {
  black: '#16191d',
  white: '#e6e6e6',
  red:   '#e5534b',
  green: '#57ab5a',
  blue:  '#539bf5',
} as const;

/** White at reduced opacity — the ONLY source of visual hierarchy. */
export const w = (opacity: number) => `rgba(230, 230, 230, ${opacity})`;

export const theme = createTheme({
  primaryColor: 'blue',
  primaryShade: 5,
  colors: {
    blue:  colorsTuple(TOKENS.blue),
    red:   colorsTuple(TOKENS.red),
    green: colorsTuple(TOKENS.green),
    dark: [
      TOKENS.white, TOKENS.white, w(0.60), w(0.45),
      w(0.25), w(0.15), '#1c2026', TOKENS.black, TOKENS.black, TOKENS.black,
    ],
  },
  fontFamily: 'Inter, system-ui, -apple-system, sans-serif',
  defaultRadius: 'sm',
});
