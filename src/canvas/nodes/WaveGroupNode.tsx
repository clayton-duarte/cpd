/**
 * React Flow group node that a wave's job nodes parent into. The box itself
 * mirrors WaveGroup's chrome (same border/surface tokens) but is sized by
 * ELK to its children's bounding box rather than shrinking to content, since
 * placement here is computed, not CSS-intrinsic.
 */
export function WaveGroupNode() {
  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        border: `1px solid rgba(230,230,230,.10)`,
        borderRadius: 'var(--mantine-radius-sm)',
        backgroundColor: 'rgba(230,230,230,.02)',
        boxSizing: 'border-box',
        pointerEvents: 'none',
      }}
    />
  );
}
