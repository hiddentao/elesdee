import type { Account, Session } from '../api/types';
import { accountFor } from '../state/sessionStatus';
import {
  AgentIcon,
  BranchIcon,
  FolderIcon,
  MoreIcon,
  TerminalIcon,
} from './icons';
import { StatusBadge } from './StatusBadge';
import { UsagePill } from './UsagePill';

interface SessionHeaderProps {
  session: Session;
  accounts: Account[];
  actionsOpen: boolean;
  onOpenMenu: (trigger: HTMLElement) => void;
  branchOpen: boolean;
  onOpenBranchMenu: (trigger: HTMLElement) => void;
}

export function SessionHeader({
  session,
  accounts,
  actionsOpen,
  onOpenMenu,
  branchOpen,
  onOpenBranchMenu,
}: SessionHeaderProps) {
  const account = accountFor(accounts, session.accountId);

  return (
    <header className="session-head">
      <div className="session-head-main">
        <div className="session-title-line">
          <h1 className="session-title">{session.name}</h1>
          <StatusBadge session={session} />
        </div>

        <div className="session-meta-line">
          {session.kind === 'agent' && account ? (
            <UsagePill account={account} />
          ) : (
            <span className="chip-meta">
              {session.kind === 'terminal' ? <TerminalIcon /> : <AgentIcon />}
              <span className="chip-text">
                {session.kind === 'terminal'
                  ? 'Terminal'
                  : session.client ?? 'Agent'}
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
          className="btn btn-ghost session-actions"
          aria-label="Session actions"
          aria-haspopup="menu"
          aria-expanded={actionsOpen}
          title="Session actions"
          onClick={(event) => onOpenMenu(event.currentTarget)}
        >
          <MoreIcon />
        </button>
      </div>
    </header>
  );
}
