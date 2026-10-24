import { useRef, useState } from 'react';

/** Edits a name in place: Enter or leaving the field keeps it, Escape drops the change. */
export function NameInput({
  initial,
  className,
  placeholder,
  onCommit,
  onDone,
}: {
  initial: string;
  className: string;
  placeholder?: string;
  /** Called with the trimmed name when it changed (may be "" where an empty name means something). */
  onCommit: (name: string) => void;
  onDone: () => void;
}) {
  const [value, setValue] = useState(initial);
  // Once kept or dropped, that is it: the blur that follows (focus moving on) must not keep it again.
  const settled = useRef(false);
  const finish = (keep: boolean) => {
    if (settled.current) return;
    settled.current = true;
    if (keep && value.trim() !== initial) onCommit(value.trim());
    onDone();
  };
  return (
    <input
      className={className}
      value={value}
      placeholder={placeholder}
      autoFocus
      onFocus={(event) => event.currentTarget.select()}
      onChange={(event) => setValue(event.target.value)}
      onBlur={() => finish(true)}
      // Keys typed here belong to the field, not to the drag source or the terminal around it.
      onMouseDown={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key !== 'Enter' && event.key !== 'Escape') return;
        event.preventDefault(); // no keypress for whatever gets the focus next
        finish(event.key === 'Enter');
      }}
    />
  );
}
