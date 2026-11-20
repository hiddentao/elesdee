interface IconProps {
  className?: string;
}

const base = {
  width: 16,
  height: 16,
  viewBox: '0 0 16 16',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.5,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
  focusable: false,
};

export function PlusIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M8 3v10M3 8h10" />
    </svg>
  );
}

export function ChevronDownIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="m4 6 4 4 4-4" />
    </svg>
  );
}

export function ChevronRightIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="m6 4 4 4-4 4" />
    </svg>
  );
}

export function FolderIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.2l1.3 1.5h5.5A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5Z" />
    </svg>
  );
}

export function FileIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M4 2.5h5l3 3v8H4Z" />
      <path d="M9 2.5v3h3" />
    </svg>
  );
}

export function PanelIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <path d="M10 3v10" />
    </svg>
  );
}

export function BranchIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="4.5" cy="4" r="1.5" />
      <circle cx="4.5" cy="12" r="1.5" />
      <circle cx="11.5" cy="6" r="1.5" />
      <path d="M4.5 5.5v5M11.5 7.5c0 2-2 2.8-4 3.2" />
    </svg>
  );
}

export function TerminalIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <path d="m5 6.5 2 1.8-2 1.7M8.5 10.5h2.5" />
    </svg>
  );
}

export function AgentIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M8 2.5v2M3.5 8h2M10.5 8h2M6 11h4" />
      <rect x="4.5" y="4.5" width="7" height="5" rx="1.5" />
      <circle cx="8" cy="13" r="1.2" />
    </svg>
  );
}

export function SendIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M13.5 2.5 7.5 8.5M13.5 2.5 9.5 13.5l-2-5-5-2Z" />
    </svg>
  );
}

export function AlertIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M8 2.8 14 13H2Z" />
      <path d="M8 6.3v3M8 11.2v.1" />
    </svg>
  );
}

export function ChatIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M2.5 3.5h11v7h-6l-3 2.5v-2.5h-2Z" />
    </svg>
  );
}

export function MoreIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="3.5" cy="8" r="1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="8" r="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function PresetIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M3 3.5h10v9H3Z" />
      <path d="M5 6h6M5 8.5h6M5 11h3.5" />
    </svg>
  );
}

export function LayersIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="m8 2.5 5.5 2.8L8 8 2.5 5.3 8 2.5Z" />
      <path d="m2.5 8 5.5 2.8L13.5 8M2.5 10.7 8 13.5l5.5-2.8" />
    </svg>
  );
}

export function ListIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M5.5 4h8M5.5 8h8M5.5 12h8" />
      <circle cx="2.8" cy="4" r=".7" fill="currentColor" stroke="none" />
      <circle cx="2.8" cy="8" r=".7" fill="currentColor" stroke="none" />
      <circle cx="2.8" cy="12" r=".7" fill="currentColor" stroke="none" />
    </svg>
  );
}


export function PencilIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="m3 13 .7-3.2 8.2-8.2 2.5 2.5-8.2 8.2L3 13Z" />
      <path d="m10.6 2.9 2.5 2.5" />
    </svg>
  );
}

export function CopyIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
      <path d="M10.5 5.5v-2A1.5 1.5 0 0 0 9 2H3.5A1.5 1.5 0 0 0 2 3.5V9a1.5 1.5 0 0 0 1.5 1.5h2" />
    </svg>
  );
}

export function TrashIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M3 4.5h10M6 4.5V3h4v1.5M4.5 4.5l.6 8a1.5 1.5 0 0 0 1.5 1.4h2.8a1.5 1.5 0 0 0 1.5-1.4l.6-8" />
    </svg>
  );
}

export function ReopenIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M3 8a5 5 0 1 0 1.5-3.5" />
      <path d="M3 2.8v3.5h3.5" />
    </svg>
  );
}

export function SettingsIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="8" cy="8" r="2" />
      <path d="M6.7 2.5h2.6l.4 1.6 1.4.8 1.6-.4 1.3 2.2-1.2 1.1v1.6l1.2 1.1-1.3 2.2-1.6-.4-1.4.8-.4 1.6H6.7l-.4-1.6-1.4-.8-1.6.4-1.3-2.2 1.2-1.1V6.9L2 5.8l1.3-2.2 1.6.4 1.4-.8.4-1.6Z" />
    </svg>
  );
}

export function CloseIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="m4 4 8 8M12 4l-8 8" />
    </svg>
  );
}

export function SparkIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M8 2.5c.4 2.6 1.9 4.1 4.5 4.5-2.6.4-4.1 1.9-4.5 4.5-.4-2.6-1.9-4.1-4.5-4.5C6.1 6.6 7.6 5.1 8 2.5Z" />
    </svg>
  );
}

export function GlobeIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M2.5 8h11M8 2.5c1.6 1.7 2.4 3.5 2.4 5.5S9.6 11.8 8 13.5C6.4 11.8 5.6 10 5.6 8S6.4 4.2 8 2.5Z" />
    </svg>
  );
}

export function PowerIcon({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M8 2.5v5" />
      <path d="M5.2 4.3a5 5 0 1 0 5.6 0" />
    </svg>
  );
}
