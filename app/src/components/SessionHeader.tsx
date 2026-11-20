import type { Provider, Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { providerFor } from '../state/sessionStatus';
import {
  AgentIcon,
  BranchIcon,
  FolderIcon,
  MoreIcon,
  PanelIcon,
  TerminalIcon,
} from './icons';
import { StatusBadge } from './StatusBadge';
import { UsagePill } from './UsagePill';

interface SessionHeaderProps {
  session: Session;
  providers: Provider[];
  actionsOpen: boolean;
  onOpenMenu: (trigger: HTMLElement) => void;
  branchOpen: boolean;
  onOpenBranchMenu: (trigger: HTMLElement) => void;
  panelOpen: boolean;
  onTogglePanel: () => void;
  onOpenSwitch: () => void;
}

export function SessionHeader({
  session,
  providers,
  actionsOpen,
  onOpenMenu,
  branchOpen,
  onOpenBranchMenu,
  panelOpen,
  onTogglePanel,
  onOpenSwitch,
}: SessionHeaderProps) {
  const { t } = useLanguage();
  const provider = providerFor(providers, session.providerId);

  return (
    <header className="session-head">
      <div className="session-head-main">
        <div className="session-title-line">
          <h1 className="session-title">{session.name}</h1>
          <StatusBadge session={session} />
        </div>

        <div className="session-meta-line">
          {session.kind === 'agent' && provider ? (
            <UsagePill
              session={session}
              provider={provider}
              canSwitch={session.status === 'active'}
              onOpenSwitch={onOpenSwitch}
            />
          ) : (
            <span className="chip-meta">
              {session.kind === 'terminal' ? <TerminalIcon /> : <AgentIcon />}
              <span className="chip-text">
                {session.kind === 'terminal'
                  ? t('status.terminal')
                  : session.client ?? t('status.agent')}
              </span>
            </span>
          )}

          {session.branch ? (
            <button
              type="button"
              className="chip-meta is-mono branch-pill"
              aria-haspopup="menu"
              aria-expanded={branchOpen}
              title={session.branch}
              onClick={(event) => onOpenBranchMenu(event.currentTarget)}
            >
              <BranchIcon />
              <span className="chip-text">{session.branch}</span>
            </button>
          ) : null}

          {session.kind === 'terminal' ? (
            <span className="chip-meta is-mono" title={session.folder}>
              <FolderIcon />
              <span className="chip-text">{session.folder}</span>
            </span>
          ) : null}
        </div>
      </div>

      <div className="session-head-actions">
        <button
          type="button"
          className={`btn btn-ghost session-panel-toggle${panelOpen ? ' is-on' : ''}`}
          aria-label={t('session.panel')}
          aria-expanded={panelOpen}
          aria-controls="session-side-panel"
          title={t('session.panel')}
          onClick={onTogglePanel}
        >
          <PanelIcon />
        </button>
        <button
          type="button"
          className="btn btn-ghost session-actions"
          aria-label={t('session.headerActions')}
          aria-haspopup="menu"
          aria-expanded={actionsOpen}
          title={t('session.headerActions')}
          onClick={(event) => onOpenMenu(event.currentTarget)}
        >
          <MoreIcon />
        </button>
      </div>
    </header>
  );
}
