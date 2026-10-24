// Small inline icons (currentColor), so no icon font or asset pipeline is needed.
const base = { width: 14, height: 14, viewBox: '0 0 16 16', fill: 'none', stroke: 'currentColor', strokeWidth: 1.6 };

export const CollapseIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M10 3.5 5.5 8l4.5 4.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

export const CloseIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="m4 4 8 8M12 4l-8 8" strokeLinecap="round" />
  </svg>
);

export const PlusIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M8 3v10M3 8h10" strokeLinecap="round" />
  </svg>
);

export const MoreIcon = () => (
  <svg {...base} aria-hidden="true" stroke="none" fill="currentColor">
    {[4, 8, 12].map((x) => (
      <circle key={x} cx={x} cy={8} r={1.3} />
    ))}
  </svg>
);

export const GripIcon = () => (
  <svg {...base} aria-hidden="true" stroke="none" fill="currentColor">
    {[5, 8, 11].flatMap((y) => [6, 10].map((x) => <circle key={`${x}-${y}`} cx={x} cy={y} r={1.1} />))}
  </svg>
);
