import {
  useEffect,
  useLayoutEffect,
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
    (element) => !element.closest('[hidden], [inert]'),
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
  const initialFocusDone = useRef(false);
  const wasBusy = useRef(busy);
  const [opener] = useState<HTMLElement | null>(() =>
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );

  useEffect(() => {
    if (initialFocusDone.current || busy) {
      return;
    }
    initialFocusDone.current = true;

    const dialog = dialogRef.current;
    if (!dialog || dialog.contains(document.activeElement)) {
      return;
    }

    const focusable = focusableElements(dialog);
    (focusable[0] ?? dialog).focus();
  }, [busy]);

  useLayoutEffect(() => {
    return () => {
      queueMicrotask(() => {
        // A repeated mount keeps the dialog in the document, and another
        // focus target means the app already moved focus on purpose.
        if (dialogRef.current?.isConnected) {
          return;
        }
        const active = document.activeElement;
        if (
          active instanceof HTMLElement &&
          active !== document.body &&
          active.isConnected
        ) {
          return;
        }
        if (opener?.isConnected) {
          opener.focus();
        }
      });
    };
  }, [opener]);

  useEffect(() => {
    if (busy) {
      statusRef.current?.focus();
    }
  }, [busy]);

  useEffect(() => {
    const was = wasBusy.current;
    wasBusy.current = busy;
    if (!was || busy) {
      return;
    }

    // The busy status unmounts when the request ends. If focus was on it,
    // the browser drops focus to the page body, so put it back inside.
    const active = document.activeElement;
    if (
      active instanceof HTMLElement &&
      active !== document.body &&
      active.isConnected
    ) {
      return;
    }
    const dialog = dialogRef.current;
    if (!dialog) {
      return;
    }
    const focusable = focusableElements(dialog);
    (focusable[0] ?? dialog).focus();
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
      event.preventDefault();
      dialogRef.current?.focus();
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    const index = active instanceof HTMLElement ? focusable.indexOf(active) : -1;

    if (event.shiftKey) {
      if (index <= 0) {
        event.preventDefault();
        last.focus();
      }
    } else if (index === -1 || index === focusable.length - 1) {
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
        tabIndex={-1}
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
