import type { Account } from '../api/types';
import { Dialog } from './Dialog';
import { UsageLimitRow } from './UsageLimitRow';

interface UsageDialogProps {
  accounts: Account[];
  onClose: () => void;
}

export function UsageDialog({ accounts, onClose }: UsageDialogProps) {
  return (
    <Dialog
      titleId="usage-title"
      descriptionId="usage-description"
      onClose={onClose}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="usage-title">
          Usage
        </h2>
        <p className="dialog-sub" id="usage-description">
          Limits for every account on this computer.
        </p>
      </div>

      <div className="dialog-body usage-panel">
        {accounts.length === 0 ? (
          <p className="usage-empty">No usage data</p>
        ) : (
          accounts.map((account) => (
            <section
              className="usage-card"
              key={account.id}
              aria-label={account.name}
            >
              <div className="usage-card-head">
                <div className="usage-card-main">
                  <span className="usage-name">{account.name}</span>
                  <span className="usage-plan">
                    {account.plan} · {account.client}
                  </span>
                </div>
              </div>

              {account.rows.length === 0 ? (
                <p className="usage-empty">No usage data</p>
              ) : (
                <div className="usage-card-rows">
                  {account.rows.map((row, index) => (
                    <UsageLimitRow key={`${row.label}-${index}`} row={row} />
                  ))}
                </div>
              )}

              {account.foot ? (
                <p className="usage-foot">{account.foot}</p>
              ) : null}
            </section>
          ))
        )}
      </div>

      <div className="dialog-foot">
        <button
          type="button"
          className="btn btn-primary"
          onClick={onClose}
          autoFocus
        >
          Close
        </button>
      </div>
    </Dialog>
  );
}
