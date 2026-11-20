import {
  Fragment,
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from 'react';
import { createPortal } from 'react-dom';
import type { Project, Session } from '../api/types';
import { englishT, type Translate } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { statusText } from '../state/sessionStatus';
import { Combobox, type ComboboxLoader } from './Combobox';
import { Dialog } from './Dialog';
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
  | 'move'
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
  canMove?: boolean;
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

export function menuItems(
  session: Session,
  t: Translate = englishT,
  canMove = false,
): MenuItem[] {
  const items: MenuItem[] = [];
  const closed = session.status === 'closed';

  if (closed) {
    items.push({ action: 'reopen', label: t('session.reopen') });
  }

  items.push({ action: 'rename', label: t('session.renameMenu') });

  if (canMove) {
    items.push({ action: 'move', label: t('session.moveToProject') });
  }

  if (!closed && session.kind === 'agent') {
    items.push({
      action: 'switch-account',
      label: t('session.switchAccountMenu'),
    });
  }

  if (session.folder.trim() !== '') {
    items.push({ action: 'copy-folder', label: t('session.copyFolder') });
  }

  if (session.branch) {
    items.push({ action: 'copy-branch', label: t('session.copyBranch') });
  }

  if (closed) {
    items.push({
      action: 'delete',
      label: t('session.deleteMenu'),
      danger: true,
      separated: true,
    });
  } else {
    items.push({
      action: 'close',
      label: t('session.closeMenuAction'),
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
    case 'move':
      return <FolderIcon />;
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
  canMove = false,
  onAction,
  onClose,
}: SessionActionsMenuProps) {
  const { t } = useLanguage();
  const items = menuItems(session, t, canMove);
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
        aria-label={t('session.menuLabel', { name: session.name })}
        style={{ left, top }}
        onKeyDown={handleKeyDown}
      >
        <div className="menu-head">
          <div>
            <div className="menu-title">{session.name}</div>
            <div className="menu-sub">{statusText(session, t)}</div>
          </div>
          <button
            type="button"
            className="menu-close"
            aria-label={t('session.closeMenu')}
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

interface MoveSessionDialogProps {
  session: Session;
  projects: Project[];
  onMove: (session: Session, projectId: string) => Promise<boolean>;
  onClose: () => void;
}

/// A single-pointer way to move a session. It picks from the projects that the
/// session is not already in and runs the same move action as dragging and
/// the keyboard move.
export function MoveSessionDialog({
  session,
  projects,
  onMove,
  onClose,
}: MoveSessionDialogProps) {
  const { t } = useLanguage();
  const targets = projects.filter((project) => project.id !== session.projectId);
  const [target, setTarget] = useState<string | null>(targets[0]?.id ?? null);
  const [submitting, setSubmitting] = useState(false);
  const selectedProject =
    projects.find((project) => project.id === target) ?? null;
  const selectedOption = selectedProject
    ? {
        id: selectedProject.id,
        label: selectedProject.name,
        meta: selectedProject.folder,
      }
    : null;

  const loadTargets = useCallback<ComboboxLoader>(
    async (query) => {
      const normalized = query.trim().toLowerCase();
      return projects
        .filter((project) => project.id !== session.projectId)
        .filter(
          (project) =>
            normalized === '' ||
            project.name.toLowerCase().includes(normalized) ||
            project.folder.toLowerCase().includes(normalized),
        )
        .map((project) => ({
          id: project.id,
          label: project.name,
          meta: project.folder,
        }));
    },
    [projects, session.projectId],
  );

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!target || submitting) {
      return;
    }
    setSubmitting(true);
    try {
      await onMove(session, target);
      onClose();
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="move-session-title"
      descriptionId="move-session-description"
      busyLabel={t('session.moveBusy')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="move-session-title">
          {t('session.moveTitle', { name: session.name })}
        </h2>
        <p className="dialog-sub" id="move-session-description">
          {t('session.moveSubtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="move-session-project"
            label={t('session.moveProject')}
            value={target}
            selectedOption={selectedOption}
            loadOptions={loadTargets}
            onChange={setTarget}
            placeholder={t('session.movePlaceholder')}
            autoFocus
            disabled={submitting || targets.length === 0}
          />
          {targets.length === 0 ? (
            <p className="field-hint">{t('session.moveNone')}</p>
          ) : null}
        </div>

        <div className="dialog-foot">
          <button
            type="button"
            className="btn btn-ghost"
            onClick={onClose}
            disabled={submitting}
          >
            {t('common.cancel')}
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={submitting || !target}
          >
            {submitting ? t('session.moving') : t('session.moveConfirm')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
