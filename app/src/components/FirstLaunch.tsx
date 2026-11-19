import type { AgentCheck } from '../api/types';
import { PlusIcon, SparkIcon } from './icons';

interface FirstLaunchProps {
  agents: AgentCheck[];
  agentsChecked: boolean;
  onAddProject: () => void;
}

export function FirstLaunch({
  agents,
  agentsChecked,
  onAddProject,
}: FirstLaunchProps) {
  return (
    <div className="empty-main">
      <div className="empty-card first-run">
        <div className="empty-mark">
          <SparkIcon />
        </div>
        <h1 className="empty-title">Welcome to elesdee</h1>
        <p className="empty-text">
          Add a project to start a session on this computer. elesdee uses the
          agents you installed.
        </p>

        <ul className="agent-checks" aria-label="Agent check">
          {!agentsChecked ? (
            <li className="agent-check">
              <span className="agent-check-name">Agent check</span>
              <span className="agent-check-state">Unavailable</span>
            </li>
          ) : agents.length === 0 ? (
            <li className="agent-check">
              <span className="agent-check-name">Agent check</span>
              <span className="agent-check-state">No results</span>
            </li>
          ) : (
            agents.map((agent) => (
              <li className="agent-check" key={agent.name}>
                <span className="agent-check-name">{agent.name}</span>
                <span
                  className={
                    agent.installed
                      ? 'agent-check-state'
                      : 'agent-check-state is-missing'
                  }
                >
                  {agent.installed
                    ? agent.version || 'Installed'
                    : 'Not installed'}
                </span>
              </li>
            ))
          )}
        </ul>

        <button type="button" className="btn btn-primary" onClick={onAddProject}>
          <PlusIcon />
          Add project
        </button>
      </div>
    </div>
  );
}
