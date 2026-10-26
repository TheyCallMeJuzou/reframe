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

export const WindowIcon = () => (
  <svg {...base} aria-hidden="true">
    <rect x="2.5" y="3" width="11" height="10" rx="1.5" />
    <path d="M2.5 6h11" />
  </svg>
);

export const InfoIcon = () => (
  <svg {...base} aria-hidden="true">
    <circle cx="8" cy="8" r="6" />
    <path d="M8 7.2v4M8 4.8v.01" strokeLinecap="round" />
  </svg>
);

export const GearIcon = () => (
  <svg {...base} aria-hidden="true">
    <circle cx="8" cy="8" r="2.2" />
    <path
      d="M8 1.6v1.8M8 12.6v1.8M1.6 8h1.8M12.6 8h1.8M3.5 3.5l1.3 1.3M11.2 11.2l1.3 1.3M3.5 12.5l1.3-1.3M11.2 4.8l1.3-1.3"
      strokeLinecap="round"
    />
    <circle cx="8" cy="8" r="4.4" />
  </svg>
);

export const LayersIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M8 2.5 14 5.5 8 8.5 2 5.5z" strokeLinejoin="round" />
    <path d="m2 8.2 6 3 6-3M2 10.9l6 3 6-3" strokeLinejoin="round" />
  </svg>
);

export const ExitFullscreenIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M6 2.5V6H2.5M10 2.5V6h3.5M6 13.5V10H2.5M10 13.5V10h3.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

export const PlusIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M8 3v10M3 8h10" strokeLinecap="round" />
  </svg>
);

export const ListIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M3 4h10M3 8h10M3 12h10" strokeLinecap="round" />
  </svg>
);

export const MoreIcon = () => (
  <svg {...base} aria-hidden="true" stroke="none" fill="currentColor">
    {[4, 8, 12].map((x) => (
      <circle key={x} cx={x} cy={8} r={1.3} />
    ))}
  </svg>
);

export const ChevronUpIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M3.5 10 8 5.5l4.5 4.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

export const ChevronDownIcon = () => (
  <svg {...base} aria-hidden="true">
    <path d="M3.5 6 8 10.5 12.5 6" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

export const GripIcon = () => (
  <svg {...base} aria-hidden="true" stroke="none" fill="currentColor">
    {[5, 8, 11].flatMap((y) => [6, 10].map((x) => <circle key={`${x}-${y}`} cx={x} cy={y} r={1.1} />))}
  </svg>
);
