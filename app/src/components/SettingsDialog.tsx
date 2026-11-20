import { useEffect, useState } from 'react';
import { ApiError } from '../api/client';
import type {
  AgentCheck,
  AiConfig,
  AppSettings,
  Client,
  ClientInstall,
  CreateConfigInput,
  CreateProviderInput,
  PairedDevice,
  Provider,
  ProviderOption,
  ProviderSignIn,
  SettingsPatch,
  StartProviderSignInInput,
  UpdateConfigInput,
  UpdateProviderInput,
} from '../api/types';
import type { UpdateSnapshot } from '../api/updater';
import { formatDateTime, LANGUAGES, type Language } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { providerUsage } from '../state/usage';
import { AiConfigDialog } from './AiConfigDialog';
import { Dialog } from './Dialog';
import { ProviderDialog } from './ProviderDialog';
import { UpdatePanel } from './UpdatePanel';
import {
  CloseIcon,
  DeviceIcon,
  GlobeIcon,
  LayersIcon,
  PencilIcon,
  PlusIcon,
  PowerIcon,
  PresetIcon,
  SparkIcon,
  TrashIcon,
  WrenchIcon,
} from './icons';

type SettingsSection =
  | 'general'
  | 'providers'
  | 'configs'
  | 'languages'
  | 'devices'
  | 'diagnostics'
  | 'about';

interface SettingsDialogProps {
  providers: Provider[];
  configs: AiConfig[];
  options: ProviderOption[];
  version: string | null;
  agents: AgentCheck[];
  agentsChecked: boolean;
  onInstallClient: (client: Client) => Promise<ClientInstall>;
  onGetClientInstall: (id: string) => Promise<ClientInstall>;
  onClientInstalled: (agent: AgentCheck) => void;
  update: UpdateSnapshot | null;
  settings: AppSettings;
  startAtLoginActual: boolean | null;
  startAtLoginError: string | null;
  keepAwakeActive: boolean | null;
  keepAwakeError: string | null;
  onCreateProvider: (input: CreateProviderInput) => Promise<Provider>;
  onUpdateProvider: (
    id: string,
    input: UpdateProviderInput,
  ) => Promise<Provider>;
  onRemoveProvider: (id: string) => Promise<void>;
  onStartProviderSignIn: (
    input: StartProviderSignInInput,
  ) => Promise<ProviderSignIn>;
  onGetProviderSignIn: (id: string) => Promise<ProviderSignIn>;
  onSubmitProviderSignInCode: (
    id: string,
    code: string,
  ) => Promise<ProviderSignIn>;
  onCancelProviderSignIn: (id: string) => Promise<void>;
  onCreateConfig: (input: CreateConfigInput) => Promise<AiConfig>;
  onUpdateConfig: (
    id: string,
    input: UpdateConfigInput,
  ) => Promise<AiConfig>;
  onDeleteConfig: (id: string) => Promise<void>;
  onProviderSaved: (provider: Provider) => void;
  onConfigSaved: (config: AiConfig) => void;
  onChangeSettings: (patch: SettingsPatch) => Promise<void>;
  onLoadDevices: () => Promise<PairedDevice[]>;
  onRemoveDevice: (endpointId: string) => Promise<void>;
  onExportDiagnostics: () => Promise<string | null>;
  onCheckForUpdate: () => void;
  onInstallUpdate: () => void;
  onClose: () => void;
}

export function SettingsDialog({
  providers,
  configs,
  options,
  version,
  agents,
  agentsChecked,
  onInstallClient,
  onGetClientInstall,
  onClientInstalled,
  update,
  settings,
  startAtLoginActual,
  startAtLoginError,
  keepAwakeActive,
  keepAwakeError,
  onCreateProvider,
  onUpdateProvider,
  onRemoveProvider,
  onStartProviderSignIn,
  onGetProviderSignIn,
  onSubmitProviderSignInCode,
  onCancelProviderSignIn,
  onCreateConfig,
  onUpdateConfig,
  onDeleteConfig,
  onProviderSaved,
  onConfigSaved,
  onChangeSettings,
  onLoadDevices,
  onRemoveDevice,
  onExportDiagnostics,
  onCheckForUpdate,
  onInstallUpdate,
  onClose,
}: SettingsDialogProps) {
  const { t, language, setLanguage } = useLanguage();
  const [section, setSection] = useState<SettingsSection>('providers');
  const [savingSettings, setSavingSettings] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [devicesState, setDevicesState] = useState<
    'idle' | 'loading' | 'ready' | 'error'
  >('idle');
  const [deviceAttempt, setDeviceAttempt] = useState(0);
  const [removeDeviceTarget, setRemoveDeviceTarget] =
    useState<PairedDevice | null>(null);
  const [removingDevice, setRemovingDevice] = useState(false);
  const [removeDeviceError, setRemoveDeviceError] = useState<string | null>(
    null,
  );
  const [exporting, setExporting] = useState(false);
  const [exportPath, setExportPath] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const [addProviderOpen, setAddProviderOpen] = useState(false);
  const [editProvider, setEditProvider] = useState<Provider | null>(null);
  const [removeProviderTarget, setRemoveProviderTarget] =
    useState<Provider | null>(null);
  const [removeProviderError, setRemoveProviderError] = useState<string | null>(
    null,
  );
  const [removingProvider, setRemovingProvider] = useState(false);
  const [configEditor, setConfigEditor] = useState<AiConfig | null | undefined>(
    undefined,
  );
  const [deleteConfigTarget, setDeleteConfigTarget] =
    useState<AiConfig | null>(null);
  const [deleteConfigError, setDeleteConfigError] = useState<string | null>(
    null,
  );
  const [deletingConfig, setDeletingConfig] = useState(false);
  const [configProviderId, setConfigProviderId] = useState<string | null>(null);

  async function confirmRemoveProvider() {
    if (!removeProviderTarget) {
      return;
    }
    setRemovingProvider(true);
    setRemoveProviderError(null);
    try {
      await onRemoveProvider(removeProviderTarget.id);
      setRemoveProviderTarget(null);
    } catch (error) {
      setRemoveProviderError(
        error instanceof ApiError
          ? error.message
          : t('removeAccount.error'),
      );
    } finally {
      setRemovingProvider(false);
    }
  }

  useEffect(() => {
    if (section !== 'devices') {
      return;
    }
    let cancelled = false;
    const refresh = async (initial: boolean) => {
      if (initial) {
        setDevicesState('loading');
      }
      try {
        const list = await onLoadDevices();
        if (!cancelled) {
          setDevices(list);
          setDevicesState('ready');
        }
      } catch {
        if (!cancelled && initial) {
          setDevicesState('error');
        }
      }
    };
    void refresh(true);
    const timer = window.setInterval(() => void refresh(false), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [section, onLoadDevices, deviceAttempt]);

  async function changeSetting(patch: SettingsPatch) {
    setSavingSettings(true);
    setSettingsError(null);
    try {
      await onChangeSettings(patch);
    } catch (error) {
      setSettingsError(
        error instanceof ApiError ? error.message : t('general.saveError'),
      );
    } finally {
      setSavingSettings(false);
    }
  }

  async function confirmRemoveDevice() {
    if (!removeDeviceTarget) {
      return;
    }
    setRemovingDevice(true);
    setRemoveDeviceError(null);
    try {
      await onRemoveDevice(removeDeviceTarget.endpointId);
      setDevices((current) =>
        current.filter(
          (device) => device.endpointId !== removeDeviceTarget.endpointId,
        ),
      );
      setRemoveDeviceTarget(null);
    } catch (error) {
      setRemoveDeviceError(
        error instanceof ApiError ? error.message : t('devices.removeError'),
      );
    } finally {
      setRemovingDevice(false);
    }
  }

  async function runExport() {
    setExporting(true);
    setExportError(null);
    setExportPath(null);
    try {
      const path = await onExportDiagnostics();
      if (path) {
        setExportPath(path);
      }
    } catch (error) {
      setExportError(
        error instanceof ApiError
          ? error.message
          : t('diagnostics.exportError'),
      );
    } finally {
      setExporting(false);
    }
  }

  async function confirmDeleteConfig() {
    if (!deleteConfigTarget) {
      return;
    }
    setDeletingConfig(true);
    setDeleteConfigError(null);
    try {
      await onDeleteConfig(deleteConfigTarget.id);
      setDeleteConfigTarget(null);
    } catch (error) {
      setDeleteConfigError(
        error instanceof ApiError ? error.message : t('presets.errorDelete'),
      );
    } finally {
      setDeletingConfig(false);
    }
  }

  const busy =
    addProviderOpen ||
    editProvider !== null ||
    removeProviderTarget !== null ||
    configEditor !== undefined ||
    deleteConfigTarget !== null ||
    removeDeviceTarget !== null;

  return (
    <>
      <Dialog
        titleId="settings-title"
        descriptionId="settings-description"
        className="settings-dialog"
        onClose={onClose}
        inert={busy}
      >
        <div className="settings-head">
          <div>
            <h2 className="settings-page-title" id="settings-title">
              {t('settings.title')}
            </h2>
            <p className="settings-page-sub" id="settings-description">
              {t('settings.subtitle')}
            </p>
          </div>
          <button
            type="button"
            className="btn btn-ghost settings-close"
            aria-label={t('settings.close')}
            onClick={onClose}
            autoFocus
          >
            <CloseIcon />
          </button>
        </div>

        <div className="settings-page-body">
          <nav className="settings-nav" aria-label={t('settings.sections')}>
            <button
              type="button"
              className={
                section === 'general'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'general' ? 'page' : undefined}
              onClick={() => setSection('general')}
            >
              <PowerIcon />
              <span>{t('settings.general')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'providers'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'providers' ? 'page' : undefined}
              onClick={() => setSection('providers')}
            >
              <LayersIcon />
              <span>{t('settings.providers')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'configs'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'configs' ? 'page' : undefined}
              onClick={() => setSection('configs')}
            >
              <PresetIcon />
              <span>{t('settings.configs')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'languages'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'languages' ? 'page' : undefined}
              onClick={() => setSection('languages')}
            >
              <GlobeIcon />
              <span>{t('settings.languages')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'devices'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'devices' ? 'page' : undefined}
              onClick={() => setSection('devices')}
            >
              <DeviceIcon />
              <span>{t('settings.devices')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'diagnostics'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'diagnostics' ? 'page' : undefined}
              onClick={() => setSection('diagnostics')}
            >
              <WrenchIcon />
              <span>{t('settings.diagnostics')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'about'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'about' ? 'page' : undefined}
              onClick={() => setSection('about')}
            >
              <SparkIcon />
              <span>{t('settings.about')}</span>
            </button>
          </nav>

          <div className="settings-content">
            {section === 'general' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-general-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-general-title"
                >
                  {t('settings.general')}
                </h3>
                <p className="settings-section-note">
                  {t('settings.generalNote')}
                </p>

                <div className="settings-toggle-row">
                  <div className="settings-toggle-text">
                    <span
                      className="settings-toggle-label"
                      id="general-start-label"
                    >
                      {t('general.startAtLogin')}
                    </span>
                    <p
                      className="settings-toggle-note"
                      id="general-start-note"
                    >
                      {t('general.startAtLoginNote')}
                    </p>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    className={
                      settings.startAtLogin
                        ? 'settings-switch is-on'
                        : 'settings-switch'
                    }
                    aria-checked={settings.startAtLogin}
                    aria-labelledby="general-start-label"
                    aria-describedby="general-start-note"
                    disabled={savingSettings}
                    onClick={() =>
                      void changeSetting({
                        startAtLogin: !settings.startAtLogin,
                      })
                    }
                  >
                    <span className="settings-switch-thumb" />
                  </button>
                </div>
                {startAtLoginError ? (
                  <p className="form-error" role="alert">
                    {t('general.startAtLoginRefused', {
                      message: startAtLoginError,
                    })}
                  </p>
                ) : startAtLoginActual !== null &&
                  startAtLoginActual !== settings.startAtLogin ? (
                  <p className="settings-status is-warn" role="status">
                    {t('general.startAtLoginMismatch')}
                  </p>
                ) : null}

                <div className="settings-toggle-row">
                  <div className="settings-toggle-text">
                    <span
                      className="settings-toggle-label"
                      id="general-awake-label"
                    >
                      {t('general.keepAwake')}
                    </span>
                    <p
                      className="settings-toggle-note"
                      id="general-awake-note"
                    >
                      {t('general.keepAwakeNote')}
                    </p>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    className={
                      settings.keepAwake
                        ? 'settings-switch is-on'
                        : 'settings-switch'
                    }
                    aria-checked={settings.keepAwake}
                    aria-labelledby="general-awake-label"
                    aria-describedby="general-awake-note"
                    disabled={savingSettings}
                    onClick={() =>
                      void changeSetting({ keepAwake: !settings.keepAwake })
                    }
                  >
                    <span className="settings-switch-thumb" />
                  </button>
                </div>
                {keepAwakeError ? (
                  <p className="form-error" role="alert">
                    {t('general.keepAwakeRefused', {
                      message: keepAwakeError,
                    })}
                  </p>
                ) : settings.keepAwake ? (
                  <p className="settings-status" role="status">
                    {keepAwakeActive
                      ? t('general.keepAwakeActive')
                      : t('general.keepAwakeIdle')}
                  </p>
                ) : null}
                {settingsError ? (
                  <p className="form-error" role="alert">
                    {settingsError}
                  </p>
                ) : null}
              </section>
            ) : section === 'providers' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-providers-title"
              >
                <div className="settings-section-head">
                  <div>
                    <h3
                      className="settings-section-title"
                      id="settings-providers-title"
                    >
                      {t('settings.providers')}
                    </h3>
                    <p className="settings-section-note">
                      {t('settings.providersNote')}
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => setAddProviderOpen(true)}
                  >
                    <PlusIcon />
                    {t('providers.add')}
                  </button>
                </div>

                {providers.length === 0 ? (
                  <p className="settings-empty">{t('providers.none')}</p>
                ) : (
                  <ul className="settings-accounts">
                    {providers.map((provider) => {
                      const usage = providerUsage(provider, language);
                      const used = configs.filter(
                        (config) => config.providerId === provider.id,
                      ).length;
                      return (
                        <li key={provider.id} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {provider.name}
                              </span>
                              {provider.sample ? (
                                <span className="sample-tag">
                                  {t('common.sample')}
                                </span>
                              ) : null}
                              {!provider.sample &&
                              provider.auth === 'subscription' ? (
                                <span
                                  className={
                                    provider.connected
                                      ? 'connection-tag is-connected'
                                      : 'connection-tag'
                                  }
                                >
                                  {provider.connected
                                    ? t('providers.signInConnected')
                                    : t('providers.signInNotConnected')}
                                </span>
                              ) : null}
                            </div>
                            <span className="settings-account-sub">
                              {provider.supportedClients.join(' · ')} ·{' '}
                              {provider.plan}
                            </span>
                            <span className="settings-account-sub">
                              {t('providers.usedBy', { count: used })}
                            </span>
                          </div>
                          <span
                            className={
                              usage.warn
                                ? 'settings-account-usage is-warn'
                                : 'settings-account-usage'
                            }
                          >
                            {usage.summary}
                          </span>
                          <div className="settings-account-actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('settings.editAccount', {
                                name: provider.name,
                              })}
                              onClick={() => setEditProvider(provider)}
                            >
                              <PencilIcon />
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('settings.removeAccount', {
                                name: provider.name,
                              })}
                              onClick={() => {
                                setRemoveProviderError(null);
                                setRemoveProviderTarget(provider);
                              }}
                            >
                              <TrashIcon />
                            </button>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            ) : section === 'configs' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-configs-title"
              >
                <div className="settings-section-head">
                  <div>
                    <h3
                      className="settings-section-title"
                      id="settings-configs-title"
                    >
                      {t('settings.configs')}
                    </h3>
                    <p className="settings-section-note">
                      {t('settings.configsNote')}
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => {
                      setConfigProviderId(null);
                      setConfigEditor(null);
                    }}
                  >
                    <PlusIcon />
                    {t('configs.add')}
                  </button>
                </div>

                {configs.length === 0 ? (
                  <p className="settings-empty">{t('configs.none')}</p>
                ) : (
                  <ul className="settings-accounts">
                    {configs.map((config) => {
                      const provider =
                        providers.find(
                          (item) => item.id === config.providerId,
                        ) ?? null;
                      const envCount = config.env.length;
                      const configCount = Object.keys(config.config).length;
                      return (
                        <li key={config.id} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {config.name}
                              </span>
                            </div>
                            <span className="settings-account-sub">
                              {provider?.name ?? config.providerId} ·{' '}
                              {config.client}
                              {config.model
                                ? ` · ${t('newSession.modelWith', {
                                    model: config.model,
                                  })}`
                                : ''}
                              {config.effort
                                ? ` · ${t('configs.effortLabel', {
                                    effort: config.effort,
                                  })}`
                                : ''}
                            </span>
                            <span className="settings-account-sub">
                              {envCount > 0
                                ? t(
                                    envCount === 1
                                      ? 'newSession.envOverrideOne'
                                      : 'newSession.envOverrideMany',
                                    { count: envCount },
                                  )
                                : ''}
                              {envCount > 0 && configCount > 0 ? ' · ' : ''}
                              {configCount > 0
                                ? t(
                                    configCount === 1
                                      ? 'newSession.configOverrideOne'
                                      : 'newSession.configOverrideMany',
                                    { count: configCount },
                                  )
                                : ''}
                              {envCount === 0 && configCount === 0
                                ? t('newSession.overridesNone')
                                : ''}
                            </span>
                          </div>
                          <div className="settings-account-actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('presets.edit', {
                                name: config.name,
                              })}
                              onClick={() => {
                                setConfigProviderId(null);
                                setConfigEditor(config);
                              }}
                            >
                              <PencilIcon />
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('presets.delete', {
                                name: config.name,
                              })}
                              onClick={() => {
                                setDeleteConfigError(null);
                                setDeleteConfigTarget(config);
                              }}
                            >
                              <TrashIcon />
                            </button>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            ) : section === 'languages' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-languages-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-languages-title"
                >
                  {t('settings.languages')}
                </h3>
                <div className="field settings-language">
                  <label htmlFor="settings-language">
                    {t('settings.languageLabel')}
                  </label>
                  <select
                    id="settings-language"
                    className="text-input"
                    value={language}
                    onChange={(event) =>
                      setLanguage(event.target.value as Language)
                    }
                  >
                    {LANGUAGES.map((item) => (
                      <option key={item.code} value={item.code}>
                        {item.label}
                      </option>
                    ))}
                  </select>
                  <p className="field-hint">{t('settings.languageNote')}</p>
                </div>
              </section>
            ) : section === 'devices' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-devices-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-devices-title"
                >
                  {t('settings.devices')}
                </h3>
                <p className="settings-section-note">{t('devices.note')}</p>

                {devicesState === 'loading' ? (
                  <p className="settings-empty">{t('devices.loading')}</p>
                ) : devicesState === 'error' ? (
                  <div className="settings-empty">
                    <p className="form-error" role="alert">
                      {t('devices.loadError')}
                    </p>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => setDeviceAttempt((count) => count + 1)}
                    >
                      {t('common.retry')}
                    </button>
                  </div>
                ) : devices.length === 0 ? (
                  <div className="settings-empty">
                    <p>{t('devices.empty')}</p>
                    <p className="settings-empty-hint">
                      {t('devices.emptyHint')}
                    </p>
                  </div>
                ) : (
                  <ul className="settings-accounts">
                    {devices.map((device) => {
                      const seen = formatDateTime(language, device.lastSeen);
                      return (
                        <li key={device.endpointId} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {device.name}
                              </span>
                              <span
                                className={
                                  device.connected
                                    ? 'device-state is-on'
                                    : 'device-state'
                                }
                              >
                                {device.connected
                                  ? t('devices.connected')
                                  : t('devices.disconnected')}
                              </span>
                            </div>
                            <span className="settings-account-sub">
                              {device.platform} ·{' '}
                              {seen
                                ? t('devices.lastSeen', { time: seen })
                                : t('devices.neverSeen')}
                            </span>
                          </div>
                          <div className="settings-account-actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('devices.remove', {
                                name: device.name,
                              })}
                              onClick={() => {
                                setRemoveDeviceError(null);
                                setRemoveDeviceTarget(device);
                              }}
                            >
                              <TrashIcon />
                            </button>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            ) : section === 'diagnostics' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-diagnostics-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-diagnostics-title"
                >
                  {t('settings.diagnostics')}
                </h3>
                <p className="settings-section-note">
                  {t('diagnostics.note')}
                </p>
                <p className="settings-section-note">
                  {t('diagnostics.redaction')}
                </p>
                <div className="settings-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={() => void runExport()}
                    disabled={exporting}
                  >
                    {exporting
                      ? t('diagnostics.exporting')
                      : t('diagnostics.export')}
                  </button>
                </div>
                {exportPath ? (
                  <p className="settings-status" role="status">
                    {t('diagnostics.exported', { path: exportPath })}
                  </p>
                ) : null}
                {exportError ? (
                  <p className="form-error" role="alert">
                    {exportError}
                  </p>
                ) : null}
              </section>
            ) : (
              <section
                className="settings-section"
                aria-labelledby="settings-about-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-about-title"
                >
                  {t('settings.about')}
                </h3>
                <div className="settings-about-card">
                  <span className="settings-about-name">elesdee</span>
                  {version ? (
                    <span className="settings-about-version">
                      {t('settings.aboutVersion', { version })}
                    </span>
                  ) : null}
                </div>
                <UpdatePanel
                  update={update}
                  onCheck={onCheckForUpdate}
                  onInstall={onInstallUpdate}
                />
              </section>
            )}
          </div>
        </div>
      </Dialog>

      {addProviderOpen ? (
        <ProviderDialog
          options={options}
          onSubmit={onCreateProvider}
          onStartSignIn={onStartProviderSignIn}
          onGetSignIn={onGetProviderSignIn}
          onSubmitSignInCode={onSubmitProviderSignInCode}
          onCancelSignIn={onCancelProviderSignIn}
          onSaved={(provider) => {
            onProviderSaved(provider);
            setAddProviderOpen(false);
          }}
          onClose={() => setAddProviderOpen(false)}
        />
      ) : null}

      {editProvider ? (
        <ProviderDialog
          provider={editProvider}
          options={options}
          onSubmit={(input) =>
            onUpdateProvider(editProvider.id, input as UpdateProviderInput)
          }
          onStartSignIn={onStartProviderSignIn}
          onGetSignIn={onGetProviderSignIn}
          onSubmitSignInCode={onSubmitProviderSignInCode}
          onCancelSignIn={onCancelProviderSignIn}
          onSaved={(provider) => {
            onProviderSaved(provider);
            setEditProvider(null);
          }}
          onClose={() => setEditProvider(null)}
        />
      ) : null}

      {removeProviderTarget ? (
        <Dialog
          titleId="remove-account-title"
          descriptionId="remove-account-description"
          busyLabel={t('removeAccount.removing')}
          onClose={() => setRemoveProviderTarget(null)}
          busy={removingProvider}
        >
          <div className="dialog-head">
            <h2 className="dialog-title" id="remove-account-title">
              {t('removeAccount.title')}
            </h2>
            <p className="dialog-sub" id="remove-account-description">
              {t('removeAccount.text', { name: removeProviderTarget.name })}
            </p>
          </div>
          <div className="dialog-body">
            {removeProviderError ? (
              <p className="form-error" role="alert">
                {removeProviderError}
              </p>
            ) : null}
          </div>
          <div className="dialog-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setRemoveProviderTarget(null)}
              disabled={removingProvider}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmRemoveProvider()}
              disabled={removingProvider}
            >
              {removingProvider
                ? t('removeAccount.removing')
                : t('removeAccount.confirm')}
            </button>
          </div>
        </Dialog>
      ) : null}

      {removeDeviceTarget ? (
        <Dialog
          titleId="remove-device-title"
          descriptionId="remove-device-description"
          busyLabel={t('devices.removing')}
          onClose={() => setRemoveDeviceTarget(null)}
          busy={removingDevice}
        >
          <div className="dialog-head">
            <h2 className="dialog-title" id="remove-device-title">
              {t('devices.removeTitle')}
            </h2>
            <p className="dialog-sub" id="remove-device-description">
              {t('devices.removeText', { name: removeDeviceTarget.name })}
            </p>
          </div>
          <div className="dialog-body">
            {removeDeviceError ? (
              <p className="form-error" role="alert">
                {removeDeviceError}
              </p>
            ) : null}
          </div>
          <div className="dialog-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setRemoveDeviceTarget(null)}
              disabled={removingDevice}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmRemoveDevice()}
              disabled={removingDevice}
            >
              {removingDevice
                ? t('devices.removing')
                : t('devices.removeConfirm')}
            </button>
          </div>
        </Dialog>
      ) : null}

      {configEditor !== undefined ? (
        <AiConfigDialog
          config={configEditor}
          providers={providers}
          options={options}
          agents={agents}
          agentsChecked={agentsChecked}
          onInstallClient={onInstallClient}
          onGetClientInstall={onGetClientInstall}
          onClientInstalled={onClientInstalled}
          initialProviderId={configProviderId}
          onSaveConfig={(id, input) =>
            id
              ? onUpdateConfig(id, input as UpdateConfigInput)
              : onCreateConfig(input as CreateConfigInput)
          }
          onCreateProvider={onCreateProvider}
          onStartProviderSignIn={onStartProviderSignIn}
          onGetProviderSignIn={onGetProviderSignIn}
          onSubmitProviderSignInCode={onSubmitProviderSignInCode}
          onCancelProviderSignIn={onCancelProviderSignIn}
          onProviderSaved={onProviderSaved}
          onSaved={(config) => {
            onConfigSaved(config);
            setConfigEditor(undefined);
          }}
          onClose={() => setConfigEditor(undefined)}
        />
      ) : null}

      {deleteConfigTarget ? (
        <Dialog
          titleId="delete-preset-title"
          descriptionId="delete-preset-description"
          busyLabel={t('presets.deleting')}
          onClose={() => setDeleteConfigTarget(null)}
          busy={deletingConfig}
        >
          <div className="dialog-head">
            <h2 className="dialog-title" id="delete-preset-title">
              {t('presets.deleteTitle')}
            </h2>
            <p className="dialog-sub" id="delete-preset-description">
              {t('presets.deleteText', { name: deleteConfigTarget.name })}
            </p>
          </div>
          <div className="dialog-body">
            {deleteConfigError ? (
              <p className="form-error" role="alert">
                {deleteConfigError}
              </p>
            ) : null}
          </div>
          <div className="dialog-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setDeleteConfigTarget(null)}
              disabled={deletingConfig}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmDeleteConfig()}
              disabled={deletingConfig}
            >
              {deletingConfig
                ? t('presets.deleting')
                : t('presets.deleteConfirm')}
            </button>
          </div>
        </Dialog>
      ) : null}
    </>
  );
}
