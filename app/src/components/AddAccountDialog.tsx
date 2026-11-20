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

interface AddAccountDialogProps {
  onCreate: (input: CreateAccountInput) => Promise<Account>;
  onCreated: (account: Account) => void;
  onClose: () => void;
}

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

export function AddAccountDialog({
  onCreate,
  onCreated,
  onClose,
}: AddAccountDialogProps) {
  const { t } = useLanguage();
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providerId, setProviderId] = useState('anthropic');
  const [auth, setAuth] = useState<'subscription' | 'key' | 'local'>(
    'subscription',
  );
  const [client, setClient] = useState<Client>('Claude Code');
  const [apiStyle, setApiStyle] = useState<'openai' | 'anthropic'>('anthropic');
  const [name, setName] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [server, setServer] = useState('');
  const [model, setModel] = useState('');
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
  }, [t]);

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

  function chooseAuth(next: 'subscription' | 'key' | 'local') {
    setAuth(next);
    setErrors((current) => ({ ...current, key: undefined, server: undefined }));
  }

  function chooseStyle(next: 'openai' | 'anthropic') {
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
    if (showKey && apiKey.trim() === '') {
      nextErrors.key = t('addAccount.errorKey');
    }
    if (showServer && server.trim() === '') {
      nextErrors.server = t('addAccount.errorServer');
    }

    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      setFocusTarget(
        nextErrors.name
          ? 'add-account-name'
          : nextErrors.key
            ? 'add-account-key'
            : 'add-account-server',
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
    if (showKey) {
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
      const account = await onCreate(input);
      onCreated(account);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : t('addAccount.errorAdd'),
      });
      setSubmitting(false);
    }
  }

  function describedBy(field: 'name' | 'key' | 'server') {
    return errors[field] ? `add-account-${field}-error` : undefined;
  }

  return (
    <Dialog
      titleId="add-account-title"
      descriptionId="add-account-description"
      busyLabel={t('addAccount.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="add-account-title">
          {t('addAccount.title')}
        </h2>
        <p className="dialog-sub" id="add-account-description">
          {t('addAccount.subtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <div className="field">
            <label htmlFor="add-account-provider">
              {t('addAccount.provider')}
            </label>
            <select
              id="add-account-provider"
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
              {(provider?.auth ?? ['key']).map((option) => (
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
            <label htmlFor="add-account-name">
              {t('addAccount.accountName')}
            </label>
            <input
              id="add-account-name"
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
              <p className="field-error" id="add-account-name-error" role="alert">
                {errors.name}
              </p>
            ) : null}
          </div>

          {showKey ? (
            <div className="field">
              <label htmlFor="add-account-key">
                {t('addAccount.apiKeyLabel')}
              </label>
              <input
                id="add-account-key"
                className="text-input"
                type="password"
                value={apiKey}
                autoComplete="off"
                required
                aria-invalid={errors.key ? true : undefined}
                aria-describedby={describedBy('key')}
                onChange={(event) => {
                  setApiKey(event.target.value);
                  setErrors((current) => ({ ...current, key: undefined }));
                }}
              />
              {errors.key ? (
                <p className="field-error" id="add-account-key-error" role="alert">
                  {errors.key}
                </p>
              ) : null}
            </div>
          ) : null}

          {showServer ? (
            <div className="field">
              <label htmlFor="add-account-server">
                {t('addAccount.serverUrl')}
              </label>
              <input
                id="add-account-server"
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
                <p
                  className="field-error"
                  id="add-account-server-error"
                  role="alert"
                >
                  {errors.server}
                </p>
              ) : null}
            </div>
          ) : null}

          <div className="field">
            <label htmlFor="add-account-model">
              {t('addAccount.model')}
            </label>
            <input
              id="add-account-model"
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
            {submitting ? t('addAccount.submitting') : t('addAccount.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
