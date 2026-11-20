import type { AgentCheck } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { shortVersion } from '../state/clients';
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
  const { t } = useLanguage();
  return (
    <div className="empty-main">
      <div className="empty-card first-run">
        <div className="empty-mark">
          <SparkIcon />
        </div>
        <h1 className="empty-title">{t('firstRun.title')}</h1>
        <p className="empty-text">{t('firstRun.text')}</p>

        <ul className="agent-checks" aria-label={t('firstRun.agentCheck')}>
          {!agentsChecked ? (
            <li className="agent-check">
              <span className="agent-check-name">
                {t('firstRun.agentCheck')}
              </span>
              <span className="agent-check-state">
                {t('firstRun.unavailable')}
              </span>
            </li>
          ) : agents.length === 0 ? (
            <li className="agent-check">
              <span className="agent-check-name">
                {t('firstRun.agentCheck')}
              </span>
              <span className="agent-check-state">
                {t('firstRun.noResults')}
              </span>
            </li>
          ) : (
            agents.map((agent) => {
              const version = agent.version
                ? shortVersion(agent.version)
                : '';
              return (
                <li className="agent-check" key={agent.name}>
                  <span className="agent-check-name">{agent.name}</span>
                  <span
                    className={
                      agent.installed
                        ? 'agent-check-state is-installed'
                        : 'agent-check-state is-missing'
                    }
                  >
                    {agent.installed
                      ? version
                        ? t('firstRun.installedWithVersion', { version })
                        : t('firstRun.installed')
                      : t('firstRun.notInstalled')}
                  </span>
                  {!agent.installed ? (
                    <span className="agent-check-hint">
                      {t('firstRun.installHint', { name: agent.name })}
                    </span>
                  ) : null}
                </li>
              );
            })
          )}
        </ul>

        <button type="button" className="btn btn-primary" onClick={onAddProject}>
          <PlusIcon />
          {t('firstRun.addProject')}
        </button>
      </div>
    </div>
  );
}
