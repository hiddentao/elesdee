import { useEffect, useState, type FormEvent } from 'react';
import { ApiError, listProviders } from '../api/client';
import type {
  Account,
  Client,
  CreateAccountInput,
  ProviderInfo,
} from '../api/types';
import type { MessageKey } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface AccountDialogProps {
  /** The account to change. Absent adds a new account. */
  account?: Account | null;
  onSubmit: (input: CreateAccountInput) => Promise<Account>;
  onSaved: (account: Account) => void;
  onClose: () => void;
}

type AuthMode = 'subscription' | 'key' | 'local';
type ApiStyle = 'openai' | 'anthropic';

interface FormErrors {
  name?: string;
  key?: string;
  server?: string;
  form?: string;
}

const AUTH_LABELS: Record<string, MessageKey> = {
  subscription: 'addAccount.subscription',
  key: 'addAccount.apiKey',
  local: 'addAccount.localServer',
};

const CUSTOM_CLIENTS: Record<string, Client[]> = {
  openai: ['Codex', 'OpenCode 2'],
  anthropic: ['Claude Code', 'OpenCode 2'],
};

function needsServer(provider: ProviderInfo | null, auth: string): boolean {
  if (!provider) {
    return false;
  }
  return (
    provider.custom ||
    auth === 'local' ||
    provider.id === 'ollama' ||
    provider.id === 'lmstudio'
  );
}

function styleFor(account: Account | null | undefined): ApiStyle {
  if (account?.apiStyle === 'openai' || account?.apiStyle === 'anthropic') {
    return account.apiStyle;
  }
  return account?.client === 'Claude Code' ? 'anthropic' : 'openai';
}

export function AccountDialog({
  account = null,
  onSubmit,
  onSaved,
  onClose,
}: AccountDialogProps) {
  const { t } = useLanguage();
  const editing = account !== null;
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providerId, setProviderId] = useState(account?.provider ?? 'anthropic');
  const [auth, setAuth] = useState<AuthMode>(account?.auth ?? 'subscription');
  const [client, setClient] = useState<Client>(account?.client ?? 'Claude Code');
  const [apiStyle, setApiStyle] = useState<ApiStyle>(styleFor(account));
  const [name, setName] = useState(account?.name ?? '');
  const [apiKey, setApiKey] = useState('');
  const [server, setServer] = useState(account?.server ?? '');
  const [model, setModel] = useState(account?.model ?? '');
  const [errors, setErrors] = useState<FormErrors>({});
  const [submitting, setSubmitting] = useState(false);
  const [focusTarget, setFocusTarget] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void listProviders()
      .then((list) => {
        if (!active) {
          return;
        }
        setProviders(list);
        if (editing) {
          return;
        }
        const first = list.find((provider) => !provider.custom) ?? list[0];
        if (first) {
          setProviderId(first.id);
          setAuth(first.auth[0] ?? 'key');
          setClient(first.client);
        }
      })
      .catch(() => {
        if (active) {
          setErrors({ form: t('addAccount.errorProviders') });
        }
      });
    return () => {
      active = false;
    };
  }, [editing, t]);

  useEffect(() => {
    if (!focusTarget) {
      return;
    }
    document.getElementById(focusTarget)?.focus();
    setFocusTarget(null);
  }, [focusTarget]);

  const provider =
    providers.find((item) => item.id === providerId) ?? null;
  const customClients =
    provider?.custom ? (CUSTOM_CLIENTS[apiStyle] ?? []) : [];
  const showKey = auth === 'key';
  const showServer = needsServer(provider, auth);
  const subscriptionHint =
    auth === 'subscription'
      ? t('addAccount.subscriptionHint', { client })
      : undefined;

  function chooseProvider(id: string) {
    const next = providers.find((item) => item.id === id) ?? null;
    setProviderId(id);
    setApiStyle('anthropic');
    setErrors({});
    if (!next) {
      return;
    }
    setAuth(next.auth[0] ?? 'key');
    setClient(next.custom ? 'OpenCode 2' : next.client);
    setModel('');
    setServer('');
  }

  function chooseAuth(next: AuthMode) {
    setAuth(next);
    setErrors((current) => ({ ...current, key: undefined, server: undefined }));
  }

  function chooseStyle(next: ApiStyle) {
    setApiStyle(next);
    if (!CUSTOM_CLIENTS[next]?.includes(client)) {
      setClient(CUSTOM_CLIENTS[next]?.[0] ?? 'OpenCode 2');
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const nextErrors: FormErrors = {};
    if (name.trim() === '') {
      nextErrors.name = t('addAccount.errorName');
    }
    // A saved account already has a key. A blank field keeps it.
    if (showKey && !editing && apiKey.trim() === '') {
      nextErrors.key = t('addAccount.errorKey');
    }
    if (showServer && server.trim() === '') {
      nextErrors.server = t('addAccount.errorServer');
    }

    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      setFocusTarget(
        nextErrors.name
          ? 'account-name'
          : nextErrors.key
            ? 'account-key'
            : 'account-server',
      );
      return;
    }

    const input: CreateAccountInput = {
      name: name.trim(),
      providerId,
      auth,
    };
    if (provider?.custom) {
      input.client = client;
      input.apiStyle = apiStyle;
    }
    if (showKey && apiKey.trim() !== '') {
      input.key = apiKey.trim();
    }
    if (showServer) {
      input.server = server.trim();
    }
    if (model.trim() !== '') {
      input.model = model.trim();
    }

    setErrors({});
    setSubmitting(true);

    try {
      const saved = await onSubmit(input);
      onSaved(saved);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : t(editing ? 'editAccount.errorSave' : 'addAccount.errorAdd'),
      });
      setSubmitting(false);
    }
  }

  function describedBy(field: 'name' | 'key' | 'server') {
    return errors[field] ? `account-${field}-error` : undefined;
  }

  return (
    <Dialog
      titleId="account-dialog-title"
      descriptionId="account-dialog-description"
      busyLabel={t(
        editing ? 'editAccount.busyLabel' : 'addAccount.busyLabel',
      )}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="account-dialog-title">
          {t(editing ? 'editAccount.title' : 'addAccount.title')}
        </h2>
        <p className="dialog-sub" id="account-dialog-description">
          {t(editing ? 'editAccount.subtitle' : 'addAccount.subtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <div className="field">
            <label htmlFor="account-provider">
              {t('addAccount.provider')}
            </label>
            <select
              id="account-provider"
              className="text-input"
              value={providerId}
              onChange={(event) => chooseProvider(event.target.value)}
            >
              {providers.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
          </div>

          <fieldset className="field">
            <legend>{t('addAccount.signIn')}</legend>
            <div className="segmented">
              {(provider?.auth ?? [auth]).map((option) => (
                <label className="segment" key={option}>
                  <input
                    type="radio"
                    name="account-auth"
                    value={option}
                    checked={auth === option}
                    onChange={() => chooseAuth(option)}
                  />
                  <span>{t(AUTH_LABELS[option] ?? 'addAccount.apiKey')}</span>
                </label>
              ))}
            </div>
            {subscriptionHint ? (
              <p className="field-hint">{subscriptionHint}</p>
            ) : null}
          </fieldset>

          {provider?.custom ? (
            <>
              <fieldset className="field">
                <legend>{t('addAccount.apiStyle')}</legend>
                <div className="segmented">
                  <label className="segment">
                    <input
                      type="radio"
                      name="api-style"
                      value="anthropic"
                      checked={apiStyle === 'anthropic'}
                      onChange={() => chooseStyle('anthropic')}
                    />
                    <span>{t('addAccount.anthropicCompatible')}</span>
                  </label>
                  <label className="segment">
                    <input
                      type="radio"
                      name="api-style"
                      value="openai"
                      checked={apiStyle === 'openai'}
                      onChange={() => chooseStyle('openai')}
                    />
                    <span>{t('addAccount.openaiCompatible')}</span>
                  </label>
                </div>
              </fieldset>

              <fieldset className="field">
                <legend>{t('addAccount.client')}</legend>
                <div className="segmented">
                  {customClients.map((item) => (
                    <label className="segment" key={item}>
                      <input
                        type="radio"
                        name="account-client"
                        value={item}
                        checked={client === item}
                        onChange={() => setClient(item)}
                      />
                      <span>{item}</span>
                    </label>
                  ))}
                </div>
              </fieldset>
            </>
          ) : null}

          <div className="field">
            <label htmlFor="account-name">{t('addAccount.accountName')}</label>
            <input
              id="account-name"
              className="text-input"
              value={name}
              autoFocus
              required
              aria-invalid={errors.name ? true : undefined}
              aria-describedby={describedBy('name')}
              onChange={(event) => {
                setName(event.target.value);
                setErrors((current) => ({ ...current, name: undefined }));
              }}
            />
            {errors.name ? (
              <p className="field-error" id="account-name-error" role="alert">
                {errors.name}
              </p>
            ) : null}
          </div>

          {showKey ? (
            <div className="field">
              <label htmlFor="account-key">
                {t(
                  editing
                    ? 'editAccount.apiKeyLabel'
                    : 'addAccount.apiKeyLabel',
                )}
              </label>
              <input
                id="account-key"
                className="text-input"
                type="password"
                value={apiKey}
                autoComplete="off"
                required={!editing}
                aria-invalid={errors.key ? true : undefined}
                aria-describedby={describedBy('key')}
                onChange={(event) => {
                  setApiKey(event.target.value);
                  setErrors((current) => ({ ...current, key: undefined }));
                }}
              />
              {editing ? (
                <p className="field-hint">{t('editAccount.apiKeyHint')}</p>
              ) : null}
              {errors.key ? (
                <p className="field-error" id="account-key-error" role="alert">
                  {errors.key}
                </p>
              ) : null}
            </div>
          ) : null}

          {showServer ? (
            <div className="field">
              <label htmlFor="account-server">
                {t('addAccount.serverUrl')}
              </label>
              <input
                id="account-server"
                className="text-input"
                value={server}
                placeholder="https://gateway.example/v1"
                spellCheck={false}
                required
                aria-invalid={errors.server ? true : undefined}
                aria-describedby={describedBy('server')}
                onChange={(event) => {
                  setServer(event.target.value);
                  setErrors((current) => ({ ...current, server: undefined }));
                }}
              />
              {errors.server ? (
                <p className="field-error" id="account-server-error" role="alert">
                  {errors.server}
                </p>
              ) : null}
            </div>
          ) : null}

          <div className="field">
            <label htmlFor="account-model">{t('addAccount.model')}</label>
            <input
              id="account-model"
              className="text-input"
              value={model}
              placeholder={
                provider?.defaultModel ?? t('addAccount.defaultModel')
              }
              spellCheck={false}
              onChange={(event) => setModel(event.target.value)}
            />
          </div>

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
            onClick={onClose}
            disabled={submitting}
          >
            {t('common.cancel')}
          </button>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting
              ? t(editing ? 'editAccount.submitting' : 'addAccount.submitting')
              : t(editing ? 'editAccount.submit' : 'addAccount.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
