import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import { openUrl } from '../api/shell';
import type {
  CreateProviderInput,
  Provider,
  ProviderOption,
  ProviderSignIn,
  ProviderSignInMode,
  ProviderSignInReason,
  ProviderSignInState,
  StartProviderSignInInput,
} from '../api/types';
import type { MessageKey } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface ProviderDialogProps {
  provider?: Provider | null;
  options: ProviderOption[];
  onSubmit: (input: CreateProviderInput) => Promise<Provider>;
  onStartSignIn: (input: StartProviderSignInInput) => Promise<ProviderSignIn>;
  onGetSignIn: (id: string) => Promise<ProviderSignIn>;
  onSubmitSignInCode: (id: string, code: string) => Promise<ProviderSignIn>;
  onCancelSignIn: (id: string) => Promise<void>;
  onSaved: (provider: Provider) => void;
  onClose: () => void;
}

type AuthMode = 'subscription' | 'key' | 'local';
type ApiStyle = 'openai' | 'anthropic';

interface FormErrors {
  key?: string;
  server?: string;
  form?: string;
}

const AUTH_LABELS: Record<AuthMode, MessageKey> = {
  subscription: 'addAccount.subscription',
  key: 'addAccount.apiKey',
  local: 'addAccount.localServer',
};

const ACTIVE_STATES: ProviderSignInState[] = [
  'starting',
  'waiting',
  'verifying',
];

const REASON_KEYS: Record<ProviderSignInReason, MessageKey> = {
  clientNotFound: 'providers.signInReasonClientNotFound',
  clientError: 'providers.signInReasonClientError',
  notSignedIn: 'providers.signInReasonNotSignedIn',
  notSubscription: 'providers.signInReasonNotSubscription',
  timeout: 'providers.signInReasonTimeout',
  cancelled: 'providers.signInReasonCancelled',
  verificationFailed: 'providers.signInReasonVerification',
  internal: 'providers.signInReasonInternal',
};

function needsServer(vendor: string | null, auth: string): boolean {
  return (
    vendor === 'custom' ||
    auth === 'local' ||
    vendor === 'ollama' ||
    vendor === 'lmstudio'
  );
}

function isActive(signIn: ProviderSignIn | null): boolean {
  return signIn !== null && ACTIVE_STATES.includes(signIn.state);
}

export function ProviderDialog({
  provider = null,
  options,
  onSubmit,
  onStartSignIn,
  onGetSignIn,
  onSubmitSignInCode,
  onCancelSignIn,
  onSaved,
  onClose,
}: ProviderDialogProps) {
  const { t } = useLanguage();
  const editing = provider !== null;
  const first = options.find((option) => !option.custom) ?? options[0] ?? null;
  const [vendor, setVendor] = useState(provider?.vendor ?? first?.id ?? '');
  const [auth, setAuth] = useState<AuthMode>(provider?.auth ?? 'key');
  const [apiStyle, setApiStyle] = useState<ApiStyle>(
    provider?.apiStyle === 'anthropic' ? 'anthropic' : 'openai',
  );
  const [name, setName] = useState(provider?.name ?? '');
  const [apiKey, setApiKey] = useState('');
  const [server, setServer] = useState(provider?.server ?? '');
  const [errors, setErrors] = useState<FormErrors>({});
  const [submitting, setSubmitting] = useState(false);
  const [signIn, setSignIn] = useState<ProviderSignIn | null>(null);
  const [signInError, setSignInError] = useState<string | null>(null);
  const [code, setCode] = useState('');
  const [checkingCode, setCheckingCode] = useState(false);
  const saved = useRef(false);

  const option = options.find((item) => item.id === vendor) ?? null;
  const showKey = auth === 'key';
  const showServer = needsServer(vendor, auth) || Boolean(provider?.server);
  const subscription = auth === 'subscription' && option?.signInClient !== null;
  const signInClient = option?.signInClient ?? null;
  const signingIn = isActive(signIn);

  function finish(savedProvider: Provider) {
    if (saved.current) {
      return;
    }
    saved.current = true;
    onSaved(savedProvider);
  }

  useEffect(() => {
    if (!signIn || !ACTIVE_STATES.includes(signIn.state)) {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      onGetSignIn(signIn.id)
        .then((next) => {
          if (cancelled) {
            return;
          }
          setSignIn(next);
          if (next.state === 'success' && next.provider) {
            finish(next.provider);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setSignInError(t('providers.signInReasonInternal'));
          }
        });
    }, 400);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [signIn, onGetSignIn, t]);

  function resetSignIn() {
    if (signIn && isActive(signIn)) {
      void onCancelSignIn(signIn.id).catch(() => undefined);
    }
    setSignIn(null);
    setCode('');
    setSignInError(null);
  }

  function chooseVendor(id: string) {
    resetSignIn();
    const next = options.find((item) => item.id === id) ?? null;
    setVendor(id);
    setAuth(next?.auth[0] ?? 'key');
    setErrors({});
  }

  function chooseAuth(next: AuthMode) {
    resetSignIn();
    setAuth(next);
    setErrors({});
  }

  async function startSignIn(mode: ProviderSignInMode) {
    setErrors({});
    setSignInError(null);
    setCode('');
    saved.current = false;
    setSubmitting(true);
    try {
      const next = await onStartSignIn({
        vendor,
        name: name.trim() === '' ? undefined : name.trim(),
        providerId: provider?.id,
        mode,
      });
      setSignIn(next);
      if (next.state === 'success' && next.provider) {
        finish(next.provider);
      }
    } catch (error) {
      setSignInError(
        error instanceof ApiError
          ? error.message
          : t('providers.signInReasonInternal'),
      );
    } finally {
      setSubmitting(false);
    }
  }

  async function cancelSignIn() {
    if (signIn) {
      try {
        await onCancelSignIn(signIn.id);
      } catch {
        // The attempt may already have finished.
      }
    }
    setSignIn(null);
    setCode('');
    setSignInError(null);
  }

  function close() {
    if (signIn && isActive(signIn)) {
      void onCancelSignIn(signIn.id).catch(() => undefined);
    }
    onClose();
  }

  async function submitCode() {
    if (!signIn || code.trim() === '') {
      setSignInError(t('providers.signInCodeInvalid'));
      return;
    }
    setCheckingCode(true);
    setSignInError(null);
    try {
      const next = await onSubmitSignInCode(signIn.id, code.trim());
      setSignIn(next);
      setCode('');
      if (next.state === 'success' && next.provider) {
        finish(next.provider);
      }
    } catch (error) {
      setSignInError(
        error instanceof ApiError
          ? error.message
          : t('providers.signInReasonInternal'),
      );
    } finally {
      setCheckingCode(false);
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (subscription && !editing) {
      await startSignIn('browser');
      return;
    }

    const nextErrors: FormErrors = {};
    if (showKey && !editing && apiKey.trim() === '') {
      nextErrors.key = t('providers.errorKey');
    }
    if (showServer && server.trim() === '') {
      nextErrors.server = t('providers.errorServer');
    }
    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      return;
    }

    const input: CreateProviderInput = {
      vendor,
      auth,
    };
    if (name.trim() !== '') {
      input.name = name.trim();
    }
    if (showKey && apiKey.trim() !== '') {
      input.key = apiKey.trim();
    }
    if (showServer) {
      input.server = server.trim();
    }
    if (option?.custom) {
      input.apiStyle = apiStyle;
    }

    setErrors({});
    setSubmitting(true);
    try {
      const savedProvider = await onSubmit(input);
      onSaved(savedProvider);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : t(editing ? 'providers.errorSave' : 'providers.errorAdd'),
      });
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="provider-dialog-title"
      descriptionId="provider-dialog-description"
      busyLabel={t(editing ? 'providers.saving' : 'providers.adding')}
      onClose={close}
      busy={submitting && !signingIn}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="provider-dialog-title">
          {t(editing ? 'providers.editTitle' : 'providers.addTitle')}
        </h2>
        <p className="dialog-sub" id="provider-dialog-description">
          {t(editing ? 'providers.editSubtitle' : 'providers.addSubtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <div className="field">
            <label htmlFor="provider-vendor">{t('providers.vendor')}</label>
            <select
              id="provider-vendor"
              className="text-input"
              value={vendor}
              onChange={(event) => chooseVendor(event.target.value)}
              disabled={signingIn}
            >
              {options.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
          </div>

          <fieldset className="field">
            <legend>{t('providers.signIn')}</legend>
            <div className="segmented">
              {(option?.auth ?? [auth]).map((item) => (
                <label className="segment" key={item}>
                  <input
                    type="radio"
                    name="provider-auth"
                    value={item}
                    checked={auth === item}
                    disabled={signingIn}
                    onChange={() => chooseAuth(item)}
                  />
                  <span>{t(AUTH_LABELS[item])}</span>
                </label>
              ))}
            </div>
          </fieldset>

          {option?.custom ? (
            <fieldset className="field">
              <legend>{t('providers.apiStyle')}</legend>
              <div className="segmented">
                <label className="segment">
                  <input
                    type="radio"
                    name="provider-style"
                    value="anthropic"
                    checked={apiStyle === 'anthropic'}
                    disabled={signingIn}
                    onChange={() => setApiStyle('anthropic')}
                  />
                  <span>{t('addAccount.anthropicCompatible')}</span>
                </label>
                <label className="segment">
                  <input
                    type="radio"
                    name="provider-style"
                    value="openai"
                    checked={apiStyle === 'openai'}
                    disabled={signingIn}
                    onChange={() => setApiStyle('openai')}
                  />
                  <span>{t('addAccount.openaiCompatible')}</span>
                </label>
              </div>
            </fieldset>
          ) : null}

          <div className="field">
            <label htmlFor="provider-name">{t('providers.name')}</label>
            <input
              id="provider-name"
              className="text-input"
              value={name}
              placeholder={option?.label ?? t('providers.namePlaceholder')}
              autoFocus
              disabled={signingIn}
              onChange={(event) => setName(event.target.value)}
            />
          </div>

          {subscription ? (
            <ProviderSignInPanel
              signIn={signIn}
              client={signInClient}
              connected={provider?.connected ?? false}
              canStart={!signingIn}
              canEnterCode={signIn?.client === 'Claude Code'}
              code={code}
              checkingCode={checkingCode}
              error={signInError}
              editing={editing}
              onStart={startSignIn}
              onCodeChange={setCode}
              onSubmitCode={submitCode}
              onOpenUrl={(url) => {
                void openUrl(url);
              }}
              onRetry={() => startSignIn('browser')}
            />
          ) : null}

          {showKey ? (
            <div className="field">
              <label htmlFor="provider-key">
                {t(
                  editing
                    ? 'editAccount.apiKeyLabel'
                    : 'addAccount.apiKeyLabel',
                )}
              </label>
              <input
                id="provider-key"
                className="text-input"
                type="password"
                value={apiKey}
                autoComplete="off"
                disabled={signingIn}
                required={!editing}
                aria-invalid={errors.key ? true : undefined}
                onChange={(event) => {
                  setApiKey(event.target.value);
                  setErrors((current) => ({ ...current, key: undefined }));
                }}
              />
              {editing ? (
                <p className="field-hint">{t('editAccount.apiKeyHint')}</p>
              ) : null}
              {errors.key ? (
                <p className="field-error" role="alert">
                  {errors.key}
                </p>
              ) : null}
            </div>
          ) : null}

          {showServer ? (
            <div className="field">
              <label htmlFor="provider-server">
                {t('providers.serverUrl')}
              </label>
              <input
                id="provider-server"
                className="text-input"
                value={server}
                placeholder="https://gateway.example/v1"
                spellCheck={false}
                disabled={signingIn}
                required
                aria-invalid={errors.server ? true : undefined}
                onChange={(event) => {
                  setServer(event.target.value);
                  setErrors((current) => ({ ...current, server: undefined }));
                }}
              />
              {errors.server ? (
                <p className="field-error" role="alert">
                  {errors.server}
                </p>
              ) : null}
            </div>
          ) : null}

          {errors.form ? (
            <div className="form-error" role="alert">
              <AlertIcon />
              <span>{errors.form}</span>
            </div>
          ) : null}
        </div>

        <div className="dialog-foot">
          <button
            type="button"
            className="btn btn-ghost"
            onClick={signingIn ? cancelSignIn : close}
            disabled={submitting && !signingIn}
          >
            {signingIn ? t('providers.signInCancel') : t('common.cancel')}
          </button>
          {subscription && !editing ? (
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => startSignIn('browser')}
              disabled={submitting || signingIn || signInClient === null}
            >
              {submitting
                ? t('providers.signInStarting')
                : t('providers.signInAndAdd')}
            </button>
          ) : (
            <button
              type="submit"
              className="btn btn-primary"
              disabled={submitting}
            >
              {submitting
                ? t(editing ? 'providers.saving' : 'providers.adding')
                : t(editing ? 'providers.save' : 'providers.add')}
            </button>
          )}
        </div>
      </form>
    </Dialog>
  );
}

interface ProviderSignInPanelProps {
  signIn: ProviderSignIn | null;
  client: ProviderSignIn['client'] | null;
  connected: boolean;
  canStart: boolean;
  canEnterCode: boolean;
  code: string;
  checkingCode: boolean;
  error: string | null;
  editing: boolean;
  onStart: (mode: ProviderSignInMode) => void;
  onCodeChange: (code: string) => void;
  onSubmitCode: () => void;
  onOpenUrl: (url: string) => void;
  onRetry: () => void;
}

function ProviderSignInPanel({
  signIn,
  client,
  connected,
  canStart,
  canEnterCode,
  code,
  checkingCode,
  error,
  editing,
  onStart,
  onCodeChange,
  onSubmitCode,
  onOpenUrl,
  onRetry,
}: ProviderSignInPanelProps) {
  const { t } = useLanguage();
  const state = signIn?.state ?? null;
  const url = signIn?.url ?? null;
  const active = state !== null && ACTIVE_STATES.includes(state);

  return (
    <div className="sign-in-panel">
      {editing && !active ? (
        <div className="sign-in-head">
          <span
            className={
              connected ? 'connection-tag is-connected' : 'connection-tag'
            }
          >
            {connected
              ? t('providers.signInConnected')
              : t('providers.signInNotConnected')}
          </span>
          {client ? (
            <span className="settings-account-sub">
              {t('providers.signInThrough', { client })}
            </span>
          ) : null}
        </div>
      ) : null}

      {!active && state !== 'failed' ? (
        <p className="field-hint">
          {client
            ? t('providers.signInHint', { client })
            : t('providers.signInUnavailable')}
        </p>
      ) : null}

      {!active && state !== 'failed' && editing && client ? (
        <div className="sign-in-actions">
          <button
            type="button"
            className="btn"
            onClick={() => onStart('browser')}
            disabled={!canStart}
          >
            {connected
              ? t('providers.signInAgain')
              : t('providers.signInNow')}
          </button>
          {client === 'Codex' ? (
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => onStart('code')}
              disabled={!canStart}
            >
              {t('providers.signInCodeInstead')}
            </button>
          ) : null}
        </div>
      ) : null}

      {!editing && !active && userStartedCode(client, state) ? (
        <button
          type="button"
          className="btn btn-ghost"
          onClick={() => onStart('code')}
          disabled={!canStart}
        >
          {t('providers.signInCodeInstead')}
        </button>
      ) : null}

      {state === 'starting' ? (
        <p className="sign-in-status" role="status">
          {t('providers.signInStarting')}
        </p>
      ) : null}

      {state === 'waiting' ? (
        <div className="sign-in-status" role="status">
          <p>{t('providers.signInWaiting', { client: client ?? '' })}</p>
          {url ? (
            <button
              type="button"
              className="btn btn-ghost sign-in-url"
              onClick={() => onOpenUrl(url)}
            >
              {t('providers.signInOpen')}
            </button>
          ) : null}
          {signIn?.code ? (
            <p className="sign-in-code-value">
              {t('providers.signInCodeIssued', { code: signIn.code })}
            </p>
          ) : null}
        </div>
      ) : null}

      {state === 'waiting' && canEnterCode ? (
        <div className="sign-in-code">
          <label htmlFor="provider-signin-code">
            {t('providers.signInCode')}
          </label>
          <div className="sign-in-code-row">
            <input
              id="provider-signin-code"
              className="text-input"
              value={code}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => onCodeChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  onSubmitCode();
                }
              }}
            />
            <button
              type="button"
              className="btn"
              onClick={onSubmitCode}
              disabled={checkingCode || code.trim() === ''}
            >
              {checkingCode
                ? t('providers.signInCodeChecking')
                : t('providers.signInCodeSubmit')}
            </button>
          </div>
          <p className="field-hint">{t('providers.signInCodeHint')}</p>
        </div>
      ) : null}

      {state === 'verifying' ? (
        <p className="sign-in-status" role="status">
          {t('providers.signInVerifying')}
        </p>
      ) : null}

      {state === 'failed' ? (
        <div className="form-error" role="alert">
          <AlertIcon />
          <span>
            {signIn?.reason
              ? t(REASON_KEYS[signIn.reason])
              : t('providers.signInReasonInternal')}
          </span>
          <button type="button" className="btn btn-ghost" onClick={onRetry}>
            {t('providers.signInRetry')}
          </button>
        </div>
      ) : null}

      {error ? (
        <p className="field-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function userStartedCode(
  client: ProviderSignIn['client'] | null,
  state: string | null,
): boolean {
  return client === 'Codex' && state === null;
}
