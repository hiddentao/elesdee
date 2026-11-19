import { useEffect, useRef, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import type { Session } from '../api/types';

export type BranchMenuAction = 'copy-branch' | 'copy-folder';

interface BranchActionsMenuProps {
  session: Session;
  trigger: HTMLElement;
  onAction: (action: BranchMenuAction) => void;
  onClose: () => void;
}

const MENU_WIDTH = 230;
const MENU_HEIGHT = 120;

export function BranchActionsMenu({
  session,
  trigger,
  onAction,
  onClose,
}: BranchActionsMenuProps) {
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const rect = trigger.getBoundingClientRect();

  const left = Math.round(
    Math.min(
      Math.max(8, rect.left),
      window.innerWidth - MENU_WIDTH - 8,
    ),
  );
  const top = Math.round(
    Math.min(
      Math.max(8, rect.bottom + 4),
      window.innerHeight - MENU_HEIGHT - 8,
    ),
  );

  useEffect(() => {
    itemRefs.current[0]?.focus();
  }, []);

  function close() {
    trigger.focus();
    onClose();
  }

  function select(action: BranchMenuAction) {
    trigger.focus();
    onClose();
    onAction(action);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const items = itemRefs.current.filter(
      (item): item is HTMLButtonElement => item !== null,
    );
    if (items.length === 0) {
      return;
    }
    const current = items.indexOf(document.activeElement as HTMLButtonElement);

    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        close();
        return;
      case 'Tab':
        close();
        return;
      case 'ArrowDown':
        event.preventDefault();
        items[(current + 1) % items.length]?.focus();
        return;
      case 'ArrowUp':
        event.preventDefault();
        items[(current - 1 + items.length) % items.length]?.focus();
        return;
      case 'Home':
        event.preventDefault();
        items[0]?.focus();
        return;
      case 'End':
        event.preventDefault();
        items[items.length - 1]?.focus();
        return;
      default:
        return;
    }
  }

  return createPortal(
    <div className="menu-layer">
      <div className="menu-scrim" onClick={close} aria-hidden="true" />
      <div
        className="session-menu branch-menu"
        role="menu"
        aria-label={`Branch actions for ${session.name}`}
        style={{ left, top }}
        onKeyDown={handleKeyDown}
      >
        {session.branch ? (
          <button
            ref={(element) => {
              itemRefs.current[0] = element;
            }}
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            onClick={() => select('copy-branch')}
          >
            Copy branch name
          </button>
        ) : null}
        {session.folder.trim() !== '' ? (
          <button
            ref={(element) => {
              itemRefs.current[1] = element;
            }}
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            onClick={() => select('copy-folder')}
          >
            Copy folder path
          </button>
        ) : null}
      </div>
    </div>,
    document.body,
  );
}
