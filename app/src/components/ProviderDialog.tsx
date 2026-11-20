import { useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import type {
  CreateProviderInput,
  Provider,
  ProviderOption,
} from '../api/types';
import type { MessageKey } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface ProviderDialogProps {
  provider?: Provider | null;
  options: ProviderOption[];
  onSubmit: (input: CreateProviderInput) => Promise<Provider>;
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

function needsServer(vendor: string | null, auth: string): boolean {
  return (
    vendor === 'custom' ||
    auth === 'local' ||
    vendor === 'ollama' ||
    vendor === 'lmstudio'
  );
}

export function ProviderDialog({
  provider = null,
  options,
  onSubmit,
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

  const option = options.find((item) => item.id === vendor) ?? null;
  const showKey = auth === 'key';
  const showServer = needsServer(vendor, auth) || Boolean(provider?.server);

  function chooseVendor(id: string) {
    const next = options.find((item) => item.id === id) ?? null;
    setVendor(id);
    setAuth(next?.auth[0] ?? 'key');
    setErrors({});
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
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
      const saved = await onSubmit(input);
      onSaved(saved);
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
      onClose={onClose}
      busy={submitting}
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
                    onChange={() => {
                      setAuth(item);
                      setErrors({});
                    }}
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
              onChange={(event) => setName(event.target.value)}
            />
          </div>

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
            onClick={onClose}
            disabled={submitting}
          >
            {t('common.cancel')}
          </button>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting
              ? t(editing ? 'providers.saving' : 'providers.adding')
              : t(editing ? 'providers.save' : 'providers.add')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
