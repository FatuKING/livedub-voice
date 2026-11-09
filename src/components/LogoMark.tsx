/**
 * LiveDub Voice mark: two stops (teal and coral) joined by a dashed route arc.
 * Decorative; the app name next to it carries the meaning.
 */
function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 28 28"
      fill="none"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d="M6 21C6 11 22 17 22 7"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray="2.5 3.5"
      />
      <circle cx="6" cy="21" r="4" fill="var(--color-incoming)" />
      <circle cx="22" cy="7" r="4" fill="var(--color-outgoing)" />
    </svg>
  );
}

export default LogoMark;
