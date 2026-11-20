import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import type { Account } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { usageStatusText } from '../state/usage';
import { UsageLimitRow } from './UsageLimitRow';
import { CloseIcon } from './icons';

interface UsagePopoverProps {
  accounts: Account[];
  trigger: HTMLElement | null;
  onClose: () => void;
}

function focusableElements(container: HTMLElement): HTMLElement[] {
  const nodes = container.querySelectorAll<HTMLElement>(
    'button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
  );
  return Array.from(nodes).filter((element) => element.getClientRects().length > 0);
}

export function UsagePopover({
  accounts,
  trigger,
  onClose,
}: UsagePopoverProps) {
  const { t, language } = useLanguage();
  const panelRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const [opener] = useState<HTMLElement | null>(() =>
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );

  useEffect(() => {
    panelRef.current?.querySelector<HTMLElement>('.usage-close')?.focus();
  }, []);

  useEffect(() => {
    return () => {
      if (opener?.isConnected) {
        opener.focus();
      }
    };
  }, [opener]);

  useEffect(() => {
    function handleOutside(event: MouseEvent) {
      const target = event.target as Node;
      if (panelRef.current?.contains(target)) {
        return;
      }
      if (trigger?.contains(target)) {
        return;
      }
      onClose();
    }

    document.addEventListener('mousedown', handleOutside);
    return () => document.removeEventListener('mousedown', handleOutside);
  }, [onClose, trigger]);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      onClose();
      return;
    }

    if (event.key !== 'Tab' || !panelRef.current) {
      return;
    }

    const focusable = focusableElements(panelRef.current);
    if (focusable.length === 0) {
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div
      ref={panelRef}
      className="usage-popover"
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      onKeyDown={handleKeyDown}
    >
      <div className="usage-popover-head">
        <div>
          <h2 className="usage-popover-title" id={titleId}>
            {t('usage.title')}
          </h2>
          <p className="usage-popover-sub">{t('usage.allAccounts')}</p>
        </div>
        <button
          type="button"
          className="btn btn-ghost usage-close"
          aria-label={t('usage.close')}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>

      <div className="usage-popover-body usage-panel">
        {accounts.length === 0 ? (
          <p className="usage-empty">{t('usage.none')}</p>
        ) : (
          accounts.map((account) => {
            const status =
              usageStatusText(account, language, t) ?? account.foot;
            return (
              <section
                className="usage-card"
                key={account.id}
                aria-label={account.name}
              >
                <div className="usage-card-head">
                  <div className="usage-card-main">
                    <span className="usage-name">{account.name}</span>
                    {account.sample ? (
                      <span className="sample-tag">{t('common.sample')}</span>
                    ) : null}
                    <span className="usage-plan">
                      {account.plan} · {account.client}
                    </span>
                  </div>
                </div>

                {account.rows.length === 0 ? (
                  <p className="usage-empty">{t('usage.none')}</p>
                ) : (
                  <div className="usage-card-rows">
                    {account.rows.map((row, index) => (
                      <UsageLimitRow key={`${row.label}-${index}`} row={row} />
                    ))}
                  </div>
                )}

                {status ? <p className="usage-foot">{status}</p> : null}
              </section>
            );
          })
        )}
      </div>
    </div>
  );
}
