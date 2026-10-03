/** The "Signal" mark: a mic with two arcs, from the same 100 × 100 artwork as the phone app. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      fill="none"
      stroke="var(--accent)"
      strokeWidth={8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <g transform="translate(-4 10)">
        <rect x={31} y={14} width={20} height={36} rx={10} />
        <path d="M26 44A15 15 0 0 0 56 44M41 59V72M32 72H50" />
        <path d="M62.45 14A28 28 0 0 1 62.45 50M73.17 5A42 42 0 0 1 73.17 59" />
      </g>
    </svg>
  );
}
