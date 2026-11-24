/** Bar heights from the left, in a 28×28 box (the mark is symmetric). */
const HEIGHTS = [6, 10, 16, 22, 26, 22, 16, 10, 6];

/** Teal for the others (incoming), sun for the moment of translation, coral for you (outgoing). */
function barColor(index: number) {
  if (index < 4) return "var(--color-incoming)";
  if (index === 4) return "var(--color-sun)";
  return "var(--color-outgoing)";
}

/**
 * LiveDub Voice mark, the app icon without its tile (`src-tauri/icons/icon.svg`): a two-tone
 * voice wave, one voice that changes color as it gets translated — teal bars (others → you),
 * a sun bar in the middle, coral bars (you → others). Drawn on a 28 px grid (2 px bars, 1 px
 * gaps) so it is crisp at its default size; the colors are theme tokens, so it follows the theme.
 * Decorative; the app name next to it carries the meaning.
 */
function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 28 28"
      aria-hidden="true"
      focusable="false"
    >
      {HEIGHTS.map((height, index) => (
        <rect
          key={index}
          x={1 + index * 3}
          y={14 - height / 2}
          width={2}
          height={height}
          rx={1}
          fill={barColor(index)}
        />
      ))}
    </svg>
  );
}

export default LogoMark;
