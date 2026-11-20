import { Fragment, useEffect, useRef, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import type { Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';

export type BranchMenuAction =
  | 'copy-branch'
  | 'copy-folder'
  | 'open-folder'
  | 'open-terminal';

interface BranchActionsMenuProps {
  session: Session;
  trigger: HTMLElement;
  canOpen: boolean;
  onAction: (action: BranchMenuAction) => void;
  onClose: () => void;
}

interface MenuItem {
  action: BranchMenuAction;
  label: string;
  separated?: boolean;
}

const MENU_WIDTH = 230;
const MENU_HEIGHT = 220;

export function BranchActionsMenu({
  session,
  trigger,
  canOpen,
  onAction,
  onClose,
}: BranchActionsMenuProps) {
  const { t } = useLanguage();
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const rect = trigger.getBoundingClientRect();
  const hasFolder = session.folder.trim() !== '';

  const items: MenuItem[] = [];
  if (session.branch) {
    items.push({ action: 'copy-branch', label: t('branchMenu.copyBranch') });
  }
  if (hasFolder) {
    items.push({ action: 'copy-folder', label: t('branchMenu.copyFolder') });
  }
  if (canOpen && hasFolder) {
    items.push({
      action: 'open-folder',
      label: t('branchMenu.openFolder'),
      separated: true,
    });
    items.push({
      action: 'open-terminal',
      label: t('branchMenu.openTerminal'),
    });
  }

  const left = Math.round(
    Math.min(Math.max(8, rect.left), window.innerWidth - MENU_WIDTH - 8),
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
    const focusable = itemRefs.current
      .slice(0, items.length)
      .filter((item): item is HTMLButtonElement => item !== null);
    if (focusable.length === 0) {
      return;
    }

    const current = focusable.indexOf(
      document.activeElement as HTMLButtonElement,
    );

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
        focusable[(current + 1) % focusable.length]?.focus();
        return;
      case 'ArrowUp':
        event.preventDefault();
        focusable[(current - 1 + focusable.length) % focusable.length]?.focus();
        return;
      case 'Home':
        event.preventDefault();
        focusable[0]?.focus();
        return;
      case 'End':
        event.preventDefault();
        focusable[focusable.length - 1]?.focus();
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
        aria-label={t('branchMenu.label', { name: session.name })}
        style={{ left, top }}
        onKeyDown={handleKeyDown}
      >
        {items.map((item, index) => (
          <Fragment key={item.action}>
            {item.separated ? (
              <div className="menu-sep" role="separator" />
            ) : null}
            <button
              ref={(element) => {
                itemRefs.current[index] = element;
              }}
              type="button"
              role="menuitem"
              className="menu-item"
              tabIndex={-1}
              onClick={() => select(item.action)}
            >
              {item.label}
            </button>
          </Fragment>
        ))}
      </div>
    </div>,
    document.body,
  );
}
