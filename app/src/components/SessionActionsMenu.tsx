import {
  Fragment,
  useEffect,
  useRef,
  type KeyboardEvent,
} from 'react';
import { createPortal } from 'react-dom';
import type { Session } from '../api/types';
import { statusText } from '../state/sessionStatus';
import {
  AgentIcon,
  BranchIcon,
  CloseIcon,
  FolderIcon,
  PencilIcon,
  ReopenIcon,
  TrashIcon,
} from './icons';

export type SessionMenuAction =
  | 'rename'
  | 'switch-account'
  | 'copy-folder'
  | 'copy-branch'
  | 'close'
  | 'reopen'
  | 'delete';

interface Point {
  x: number;
  y: number;
}

interface SessionActionsMenuProps {
  session: Session;
  point: Point;
  placement: 'below-end' | 'at-point';
  trigger: HTMLElement | null;
  onAction: (action: SessionMenuAction) => void;
  onClose: () => void;
}

interface MenuItem {
  action: SessionMenuAction;
  label: string;
  danger?: boolean;
  separated?: boolean;
}

const MENU_WIDTH = 230;
const MENU_MAX_HEIGHT = 300;

export function menuItems(session: Session): MenuItem[] {
  const items: MenuItem[] = [];
  const closed = session.status === 'closed';

  if (closed) {
    items.push({ action: 'reopen', label: 'Reopen' });
  }

  items.push({ action: 'rename', label: 'Rename…' });

  if (!closed && session.kind === 'agent') {
    items.push({ action: 'switch-account', label: 'Switch account…' });
  }

  if (session.folder.trim() !== '') {
    items.push({ action: 'copy-folder', label: 'Copy folder path' });
  }

  if (session.branch) {
    items.push({ action: 'copy-branch', label: 'Copy branch name' });
  }

  if (closed) {
    items.push({
      action: 'delete',
      label: 'Delete…',
      danger: true,
      separated: true,
    });
  } else {
    items.push({
      action: 'close',
      label: 'Close…',
      danger: true,
      separated: true,
    });
  }

  return items;
}

function actionIcon(action: SessionMenuAction) {
  switch (action) {
    case 'rename':
      return <PencilIcon />;
    case 'switch-account':
      return <AgentIcon />;
    case 'copy-folder':
      return <FolderIcon />;
    case 'copy-branch':
      return <BranchIcon />;
    case 'close':
      return <CloseIcon />;
    case 'reopen':
      return <ReopenIcon />;
    case 'delete':
      return <TrashIcon />;
  }
}

export function SessionActionsMenu({
  session,
  point,
  placement,
  trigger,
  onAction,
  onClose,
}: SessionActionsMenuProps) {
  const items = menuItems(session);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const left = Math.round(
    Math.min(
      Math.max(
        8,
        placement === 'below-end' ? point.x - MENU_WIDTH : point.x,
      ),
      window.innerWidth - MENU_WIDTH - 8,
    ),
  );
  const top = Math.round(
    Math.min(
      Math.max(8, point.y),
      window.innerHeight - MENU_MAX_HEIGHT - 8,
    ),
  );

  useEffect(() => {
    itemRefs.current[0]?.focus();
  }, []);

  function close() {
    trigger?.focus();
    onClose();
  }

  function select(action: SessionMenuAction) {
    trigger?.focus();
    onClose();
    onAction(action);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const focusable = itemRefs.current.filter(
      (item): item is HTMLButtonElement => item !== null,
    );
    if (focusable.length === 0) {
      return;
    }

    const current = focusable.indexOf(document.activeElement as HTMLButtonElement);

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
        className="session-menu"
        role="menu"
        aria-label={`Actions for ${session.name}`}
        style={{ left, top }}
        onKeyDown={handleKeyDown}
      >
        <div className="menu-head">
          <div>
            <div className="menu-title">{session.name}</div>
            <div className="menu-sub">{statusText(session)}</div>
          </div>
          <button
            type="button"
            className="menu-close"
            aria-label="Close menu"
            tabIndex={-1}
            onClick={close}
          >
            <CloseIcon />
          </button>
        </div>

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
              className={item.danger ? 'menu-item danger' : 'menu-item'}
              tabIndex={-1}
              onClick={() => select(item.action)}
            >
              <span className="menu-item-icon" aria-hidden="true">
                {actionIcon(item.action)}
              </span>
              {item.label}
            </button>
          </Fragment>
        ))}
      </div>
    </div>,
    document.body,
  );
}
