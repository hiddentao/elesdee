import { useEffect, useId, useRef, useState } from 'react';
import type { Account, Session } from '../api/types';
import { accountUsage, usageStatusText } from '../state/usage';
import { useLanguage } from '../i18n/LanguageProvider';
import { UsageLimitRow } from './UsageLimitRow';

interface UsagePillProps {
  session: Session;
  account: Account;
  canSwitch: boolean;
  onOpenSwitch: () => void;
}

export function UsagePill({
  session,
  account,
  canSwitch,
  onOpenSwitch,
}: UsagePillProps) {
  const { t, language } = useLanguage();
  const usage = accountUsage(account, language);
  const status = usageStatusText(account, language, t) ?? account.foot;
  const contextLeft =
    typeof session.contextLeft === 'number' ? session.contextLeft : null;
  const tipId = useId();
  const rootRef = useRef<HTMLSpanElement>(null);
  const [pinned, setPinned] = useState(false);
  const [hovered, setHovered] = useState(false);
  const open = pinned || hovered;

  useEffect(() => {
    if (!open) {
      return;
    }

    function handleOutside(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) {
        setPinned(false);
        setHovered(false);
      }
    }

    document.addEventListener('mousedown', handleOutside);
    return () => document.removeEventListener('mousedown', handleOutside);
  }, [open]);

  function handleClick() {
    if (canSwitch) {
      setPinned(false);
      setHovered(false);
      onOpenSwitch();
      return;
    }
    if (pinned) {
      setPinned(false);
      setHovered(false);
    } else {
      setPinned(true);
    }
  }

  function close() {
    setPinned(false);
    setHovered(false);
  }

  return (
    <span
      className="provider-wrap"
      ref={rootRef}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <button
        type="button"
        className="provider-pill"
        aria-haspopup={canSwitch ? 'dialog' : undefined}
        aria-expanded={canSwitch ? undefined : open}
        aria-controls={open ? tipId : undefined}
        aria-describedby={open ? tipId : undefined}
        onFocus={() => setHovered(true)}
        onBlur={() => setHovered(false)}
        onClick={handleClick}
        onKeyDown={(event) => {
          if (event.key === 'Escape' && open) {
            event.stopPropagation();
            close();
          }
        }}
      >
        <span className="provider-client">{account.client}</span>
        <span className="provider-account">{account.name}</span>
        {usage.remaining !== null ? (
          <span
            className={
              usage.critical
                ? 'usage-bar is-critical'
                : usage.warn
                  ? 'usage-bar is-warn'
                  : 'usage-bar'
            }
            aria-hidden="true"
          >
            <span
              className="usage-bar-fill"
              style={{ width: `${usage.remaining}%` }}
            />
          </span>
        ) : (
          <span className="provider-usage">{usage.summary}</span>
        )}
      </button>

      {open ? (
        <span className="usage-tip" id={tipId} role="tooltip">
          <span className="usage-tip-head">
            <span className="usage-tip-name">{account.name}</span>
            <span className="usage-tip-plan">{account.plan}</span>
          </span>
          {account.rows.map((row, index) => (
            <UsageLimitRow key={`${row.label}-${index}`} row={row} />
          ))}
          {account.rows.length === 0 ? (
            <span className="usage-tip-note">{t('usage.none')}</span>
          ) : null}
          {status ? (
            <span className="usage-tip-foot">{status}</span>
          ) : null}
          <span className="usage-tip-context">
            <span className="usage-tip-context-head">
              <strong>{t('session.contextLeft')}</strong>
              <span>
                {contextLeft === null
                  ? t('session.contextLeftUnknown')
                  : t('usage.percentLeft', {
                      percent: Math.round(contextLeft),
                    })}
              </span>
            </span>
            {contextLeft === null ? null : (
              <span className="usage-bar" aria-hidden="true">
                <span
                  className="usage-bar-fill"
                  style={{ width: `${contextLeft}%` }}
                />
              </span>
            )}
            <span className="usage-tip-note">
              {t('session.contextCompacts')}
            </span>
          </span>
        </span>
      ) : null}
    </span>
  );
}
