import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { useLanguage } from '../i18n/LanguageProvider';

interface DialogProps {
  titleId: string;
  descriptionId?: string;
  busyLabel?: string;
  onClose: () => void;
  busy?: boolean;
  inert?: boolean;
  className?: string;
  children: ReactNode;
}

function focusableElements(container: HTMLElement): HTMLElement[] {
  const nodes = container.querySelectorAll<HTMLElement>(
    'button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
  );

  return Array.from(nodes).filter(
    (element) => element.getClientRects().length > 0,
  );
}

export function Dialog({
  titleId,
  descriptionId,
  busyLabel,
  onClose,
  busy = false,
  inert = false,
  className,
  children,
}: DialogProps) {
  const { t } = useLanguage();
  const dialogRef = useRef<HTMLDivElement>(null);
  const statusRef = useRef<HTMLParagraphElement>(null);
  const [opener] = useState<HTMLElement | null>(() =>
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );

  useEffect(() => {
    return () => {
      if (opener?.isConnected) {
        opener.focus();
      }
    };
  }, [opener]);

  useEffect(() => {
    if (busy) {
      statusRef.current?.focus();
    }
  }, [busy]);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      if (!busy) {
        onClose();
      }
      return;
    }

    if (event.key !== 'Tab') {
      return;
    }

    const focusable = dialogRef.current
      ? focusableElements(dialogRef.current)
      : [];
    if (focusable.length === 0) {
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className="dialog-layer" inert={inert}>
      <div
        className="dialog-scrim"
        onClick={() => {
          if (!busy) {
            onClose();
          }
        }}
        aria-hidden="true"
      />
      <div
        ref={dialogRef}
        className={className ? `dialog ${className}` : 'dialog'}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        aria-busy={busy}
        onKeyDown={handleKeyDown}
      >
        {children}
        {busy ? (
          <p ref={statusRef} className="sr-only" role="status" tabIndex={-1}>
            {busyLabel ?? t('dialog.working')}
          </p>
        ) : null}
      </div>
    </div>
  );
}
