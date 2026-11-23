/**
 * LiveDub Voice mark, the app icon without its tile (`src-tauri/icons/icon.svg`): two speech
 * bubbles with voice bars, teal (others → you) behind coral (you → others). The bars and the gap
 * between the bubbles are cut out in the surface color, so it follows the theme.
 * Decorative; the app name next to it carries the meaning.
 */
function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="112 124 800 800"
      fill="none"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d="M256 136H528A104 104 0 0 1 632 240V432A104 104 0 0 1 528 536H368L232 656L256 536A104 104 0 0 1 152 432V240A104 104 0 0 1 256 136Z"
        fill="var(--color-incoming)"
      />
      <path
        d="M312 220V284M392 180V324M472 212V292"
        stroke="var(--color-surface)"
        strokeWidth="48"
        strokeLinecap="round"
      />
      <path
        d="M496 392H768A104 104 0 0 1 872 496V688A104 104 0 0 1 768 792L792 912L656 792H496A104 104 0 0 1 392 688V496A104 104 0 0 1 496 392Z"
        fill="var(--color-outgoing)"
        stroke="var(--color-surface)"
        strokeWidth="40"
        paintOrder="stroke"
      />
      <path
        d="M552 552V632M632 500V684M712 540V644"
        stroke="var(--color-surface)"
        strokeWidth="48"
        strokeLinecap="round"
      />
    </svg>
  );
}

export default LogoMark;
