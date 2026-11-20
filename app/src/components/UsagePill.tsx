import { useEffect, useId, useRef, useState } from 'react';
import type { Provider, Session } from '../api/types';
import { providerUsage, usageStatusText } from '../state/usage';
import { useLanguage } from '../i18n/LanguageProvider';
import { UsageLimitRow } from './UsageLimitRow';

interface UsagePillProps {
  session: Session;
  provider: Provider;
  canSwitch: boolean;
  onOpenSwitch: () => void;
}

export function UsagePill({
  session,
  provider,
  canSwitch,
  onOpenSwitch,
}: UsagePillProps) {
  const { t, language } = useLanguage();
  const usage = providerUsage(provider, language);
  const status = usageStatusText(provider, language, t) ?? provider.foot;
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
        <span className="provider-client">{session.client}</span>
        <span className="provider-account">{provider.name}</span>
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
            <span className="usage-tip-name">{provider.name}</span>
            <span className="usage-tip-plan">{provider.plan}</span>
          </span>
          {provider.rows.map((row, index) => (
            <UsageLimitRow
              key={`${row.label}-${index}`}
              row={row}
              minimum={provider.minimumBalance}
            />
          ))}
          {provider.rows.length === 0 ? (
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
