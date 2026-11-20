import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError } from '../api/client';
import type {
  PairedDevice,
  PairingCreated,
  PairingStatus,
} from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';

/** The most phones that the core stores. The page stops pairing at this count. */
export const PAIRED_DEVICE_LIMIT = 16;

type ViewPhase =
  | 'starting'
  | 'waiting'
  | 'confirm'
  | 'paired'
  | 'denied'
  | 'expired'
  | 'cancelled'
  | 'unavailable'
  | 'start-error';

type BusyAction = 'approve' | 'deny' | null;

interface PairingDialogProps {
  onStart: () => Promise<PairingCreated>;
  onStatus: () => Promise<PairingStatus>;
  onApprove: () => Promise<PairedDevice>;
  onDeny: () => Promise<void>;
  onCancel: () => Promise<void>;
  qrUrl: (cacheBust: number) => string;
  onPaired: (device: PairedDevice | null) => void;
  onClose: () => void;
  deviceLimitReached?: boolean;
  pollMs?: number;
}

function isTerminal(phase: ViewPhase): boolean {
  return (
    phase === 'paired' ||
    phase === 'denied' ||
    phase === 'expired' ||
    phase === 'cancelled' ||
    phase === 'unavailable' ||
    phase === 'start-error'
  );
}

export function PairingDialog({
  onStart,
  onStatus,
  onApprove,
  onDeny,
  onCancel,
  qrUrl,
  onPaired,
  onClose,
  deviceLimitReached = false,
  pollMs = 1000,
}: PairingDialogProps) {
  const { t } = useLanguage();
  const [phase, setPhase] = useState<ViewPhase>('starting');
  const [created, setCreated] = useState<PairingCreated | null>(null);
  const [status, setStatus] = useState<PairingStatus | null>(null);
  const [ticket, setTicket] = useState<string | null>(null);
  const [qrKey, setQrKey] = useState(0);
  const [qrFailed, setQrFailed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pollError, setPollError] = useState(false);
  const [busy, setBusy] = useState<BusyAction>(null);
  const [now, setNow] = useState(() => Date.now());

  const attempt = useRef(0);
  const started = useRef(false);
  const startRef = useRef(onStart);
  const statusRef = useRef(onStatus);
  const pairedRef = useRef(onPaired);
  const waitRef = useRef<HTMLParagraphElement>(null);
  const approveRef = useRef<HTMLButtonElement>(null);
  const retryRef = useRef<HTMLButtonElement>(null);

  startRef.current = onStart;
  statusRef.current = onStatus;
  pairedRef.current = onPaired;

  const begin = useCallback(async () => {
    const current = ++attempt.current;
    setPhase('starting');
    setCreated(null);
    setStatus(null);
    setTicket(null);
    setQrFailed(false);
    setError(null);
    setPollError(false);
    try {
      const next = await startRef.current();
      if (current !== attempt.current) {
        return;
      }
      setCreated(next);
      setTicket(next.ticket ?? null);
      setQrKey(Date.now());
      setPhase('waiting');
    } catch (failure) {
      if (current !== attempt.current) {
        return;
      }
      setPhase(
        failure instanceof ApiError && failure.status === 503
          ? 'unavailable'
          : 'start-error',
      );
    }
  }, []);

  useEffect(() => {
    if (started.current) {
      return;
    }
    started.current = true;
    void begin();
  }, [begin]);

  useEffect(() => {
    if ((phase !== 'waiting' && phase !== 'confirm') || busy !== null) {
      return;
    }
    let cancelled = false;
    let timer = 0;
    const poll = async () => {
      try {
        const next = await statusRef.current();
        if (cancelled) {
          return;
        }
        setPollError(false);
        if (next.state === 'waiting') {
          setStatus(next);
        } else if (next.state === 'confirm') {
          setStatus(next);
          setPhase('confirm');
          setTicket(null);
        } else if (next.state === 'paired') {
          setPhase('paired');
          pairedRef.current(null);
        } else if (
          next.state === 'denied' ||
          next.state === 'expired' ||
          next.state === 'cancelled'
        ) {
          setStatus(next);
          setPhase(next.state);
          setTicket(null);
        } else {
          setStatus(next);
          setPhase('expired');
          setTicket(null);
        }
      } catch {
        if (!cancelled) {
          setPollError(true);
        }
      }
      if (!cancelled) {
        timer = window.setTimeout(poll, pollMs);
      }
    };
    timer = window.setTimeout(poll, pollMs);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [phase, pollMs, busy]);

  useEffect(() => {
    if (phase !== 'waiting' && phase !== 'confirm') {
      return;
    }
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [phase]);

  useEffect(() => {
    if (phase === 'starting' || busy !== null) {
      return;
    }
    if (phase === 'waiting') {
      waitRef.current?.focus();
    } else if (phase === 'confirm') {
      approveRef.current?.focus();
    } else if (
      phase === 'unavailable' ||
      phase === 'start-error' ||
      phase === 'denied' ||
      phase === 'expired' ||
      phase === 'cancelled'
    ) {
      retryRef.current?.focus();
    }
  }, [phase, busy]);

  async function requestClose() {
    if (phase === 'starting') {
      return;
    }
    if (phase === 'waiting' || phase === 'confirm') {
      try {
        await onCancel();
      } catch {
        setError(t('pairing.cancelError'));
        return;
      }
    } else if (phase === 'start-error') {
      void onCancel().catch(() => undefined);
    }
    onClose();
  }

  async function approve() {
    setBusy('approve');
    setError(null);
    try {
      const device = await onApprove();
      setPhase('paired');
      pairedRef.current(device);
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 409) {
        setError(t('pairing.gone'));
        try {
          const next = await statusRef.current();
          if (next.state === 'confirm') {
            setStatus(next);
          } else {
            setStatus(next);
            setPhase(
              next.state === 'waiting' || next.state === 'idle'
                ? 'expired'
                : next.state,
            );
            setTicket(null);
          }
        } catch {
          setPollError(true);
        }
      } else if (failure instanceof ApiError && failure.status === 503) {
        setError(t('pairing.unavailable'));
      } else {
        setError(t('pairing.approveError'));
      }
    } finally {
      setBusy(null);
    }
  }

  async function deny() {
    setBusy('deny');
    setError(null);
    try {
      await onDeny();
      setStatus(null);
      setTicket(null);
      setPhase('denied');
    } catch (failure) {
      setError(
        failure instanceof ApiError && failure.status === 503
          ? t('pairing.unavailable')
          : t('pairing.denyError'),
      );
    } finally {
      setBusy(null);
    }
  }

  const expiresAt = status?.expiresAt ?? created?.expiresAt;
  const secondsLeft = expiresAt
    ? Math.max(0, expiresAt - Math.floor(now / 1000))
    : null;
  const code = status?.code ?? '';
  const phoneName = status?.phoneName ?? '';
  const failed = phase === 'start-error' || phase === 'unavailable';
  const busyLabel =
    phase === 'starting'
      ? t('pairing.starting')
      : busy === 'approve'
        ? t('pairing.approving')
        : t('pairing.denying');

  function terminalMessage(): string {
    if (phase === 'paired') {
      return t('pairing.paired');
    }
    if (phase === 'denied') {
      return deviceLimitReached
        ? t('pairing.deniedFull')
        : t('pairing.denied');
    }
    if (phase === 'expired') {
      return t('pairing.expired');
    }
    if (phase === 'cancelled') {
      return t('pairing.cancelled');
    }
    if (phase === 'unavailable') {
      return t('pairing.unavailable');
    }
    return t('pairing.startError');
  }

  return (
    <Dialog
      titleId="pairing-title"
      descriptionId="pairing-description"
      className="pairing-dialog"
      busyLabel={busyLabel}
      onClose={() => void requestClose()}
      busy={phase === 'starting' || busy !== null}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="pairing-title">
          {t('pairing.title')}
        </h2>
        <p className="dialog-sub" id="pairing-description">
          {t('pairing.intro')}
        </p>
      </div>

      <div className="dialog-body">
        {phase === 'starting' ? (
          <p className="pairing-wait" role="status">
            {t('pairing.starting')}
          </p>
        ) : null}

        {phase === 'waiting' ? (
          <>
            <p
              className="pairing-wait"
              role="status"
              tabIndex={-1}
              ref={waitRef}
            >
              {t('pairing.scan')}
            </p>
            {qrFailed || !created ? (
              ticket ? (
                <div className="field pairing-ticket-field">
                  <label htmlFor="pairing-ticket">
                    {t('pairing.ticketLabel')}
                  </label>
                  <textarea
                    id="pairing-ticket"
                    className="text-input pairing-ticket"
                    value={ticket}
                    rows={3}
                    readOnly
                    spellCheck={false}
                    onFocus={(event) => event.currentTarget.select()}
                  />
                  <p className="field-hint">{t('pairing.ticketHint')}</p>
                </div>
              ) : (
                <p className="form-error" role="alert">
                  {t('pairing.qrError')}
                </p>
              )
            ) : (
              <img
                className="pairing-qr"
                src={qrUrl(qrKey)}
                alt={t('pairing.qrAlt')}
                width={240}
                height={240}
                onError={() => setQrFailed(true)}
              />
            )}
            {created && !created.relayReady ? (
              <p className="settings-status is-warn">
                {t('pairing.sameNetwork')}
              </p>
            ) : null}
            {secondsLeft !== null && secondsLeft > 0 ? (
              <p className="settings-status">
                {t('pairing.expiresIn', { seconds: secondsLeft })}
              </p>
            ) : null}
          </>
        ) : null}

        {phase === 'confirm' ? (
          <>
            <p className="pairing-phone" role="status">
              {t('pairing.phone', { name: phoneName })}
            </p>
            <p className="pairing-instruction">{t('pairing.compare')}</p>
            <p className="pairing-code-label">{t('pairing.codeLabel')}</p>
            <p
              className="pairing-code"
              aria-label={t('pairing.codeSpoken', {
                code: code.split('').join(' '),
              })}
            >
              {code}
            </p>
            {secondsLeft !== null && secondsLeft > 0 ? (
              <p className="settings-status">
                {t('pairing.expiresIn', { seconds: secondsLeft })}
              </p>
            ) : null}
          </>
        ) : null}

        {isTerminal(phase) ? (
          <p
            className={failed ? 'form-error' : 'pairing-wait'}
            role={failed ? 'alert' : 'status'}
            tabIndex={-1}
          >
            {terminalMessage()}
          </p>
        ) : null}

        {error ? (
          <p className="form-error" role="alert">
            {error}
          </p>
        ) : null}
        {pollError && phase !== 'starting' ? (
          <p className="settings-status is-warn" role="status">
            {t('pairing.statusError')}
          </p>
        ) : null}
      </div>

      <div className="dialog-foot">
        {phase === 'waiting' || phase === 'confirm' ? (
          <>
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => void (phase === 'confirm' ? deny() : requestClose())}
              disabled={busy !== null}
            >
              {phase === 'confirm' ? t('pairing.deny') : t('pairing.cancel')}
            </button>
            {phase === 'confirm' ? (
              <button
                ref={approveRef}
                type="button"
                className="btn btn-primary"
                onClick={() => void approve()}
                disabled={busy !== null}
              >
                {t('pairing.approve')}
              </button>
            ) : null}
          </>
        ) : phase === 'starting' ? null : phase === 'paired' ? (
          <button type="button" className="btn btn-primary" onClick={onClose}>
            {t('common.close')}
          </button>
        ) : (
          <>
            <button type="button" className="btn btn-ghost" onClick={onClose}>
              {t('common.close')}
            </button>
            <button
              ref={retryRef}
              type="button"
              className="btn btn-primary"
              onClick={() => void begin()}
            >
              {t('pairing.retry')}
            </button>
          </>
        )}
      </div>
    </Dialog>
  );
}
