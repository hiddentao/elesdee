import { useCallback, useState, type FormEvent } from 'react';
import { ApiError, listBranches, listConfigs } from '../api/client';
import type {
  AiConfig,
  Project,
  Provider,
  UpdateProjectDefaultsInput,
} from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { configOption } from './configOption';
import { Combobox, type ComboboxLoader, type ComboboxOption } from './Combobox';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface ProjectSettingsDialogProps {
  project: Project;
  configs: AiConfig[];
  providers: Provider[];
  onSave: (input: UpdateProjectDefaultsInput) => Promise<void>;
  onClose: () => void;
}

function branchOption(
  branch: { name: string; current: boolean },
  currentLabel: string,
): ComboboxOption {
  return {
    id: branch.name,
    label: branch.name,
    tag: branch.current ? currentLabel : undefined,
  };
}

export function ProjectSettingsDialog({
  project,
  configs,
  providers,
  onSave,
  onClose,
}: ProjectSettingsDialogProps) {
  const { t, language } = useLanguage();
  const initialConfig = project.defaultConfigId
    ? configs.find((config) => config.id === project.defaultConfigId) ?? null
    : null;
  const [configId, setConfigId] = useState<string | null>(
    initialConfig?.id ?? null,
  );
  const [branch, setBranch] = useState<string | null>(
    project.defaultSourceBranch ?? null,
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedConfig = configId
    ? configs.find((config) => config.id === configId) ?? null
    : null;

  const loadConfigOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listConfigs(query, signal);
      return list.map((config) => {
        const provider =
          providers.find((item) => item.id === config.providerId) ?? null;
        return {
          ...configOption(config, providers, language),
          tag: provider?.sample ? t('common.sample') : undefined,
        };
      });
    },
    [providers, language, t],
  );

  const loadBranchOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listBranches(project.folder, query, signal);
      return list.map((branch) =>
        branchOption(branch, t('newSession.currentTag')),
      );
    },
    [project.folder, t],
  );

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    setSubmitting(true);
    setError(null);
    try {
      await onSave({
        defaultConfigId: configId,
        defaultSourceBranch: branch,
      });
      onClose();
    } catch (failure) {
      setSubmitting(false);
      setError(
        failure instanceof ApiError
          ? failure.message
          : t('projectSettings.errorSave'),
      );
    }
  }

  return (
    <Dialog
      titleId="project-settings-title"
      descriptionId="project-settings-description"
      busyLabel={t('projectSettings.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="project-settings-title">
          {t('projectSettings.title')}
        </h2>
        <p className="dialog-sub" id="project-settings-description">
          {project.name} · {project.folder}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="project-default-config"
            label={t('projectSettings.defaultConfig')}
            value={configId}
            loadOptions={loadConfigOptions}
            selectedOption={
              selectedConfig
                ? {
                    ...configOption(selectedConfig, providers, language),
                    tag: providers.find(
                      (provider) => provider.id === selectedConfig.providerId,
                    )?.sample
                      ? t('common.sample')
                      : undefined,
                  }
                : null
            }
            onChange={setConfigId}
            placeholder={t('projectSettings.noDefaultConfig')}
            hint={t('projectSettings.configHint')}
            autoFocus
          />
          {configId ? (
            <button
              type="button"
              className="link-button project-default-clear"
              onClick={() => setConfigId(null)}
            >
              {t('projectSettings.clearConfig')}
            </button>
          ) : null}

          <Combobox
            key={project.folder}
            id="project-default-branch"
            label={t('projectSettings.defaultBranch')}
            value={branch}
            loadOptions={loadBranchOptions}
            selectedOption={
              branch ? { id: branch, label: branch } : null
            }
            onChange={setBranch}
            placeholder={t('projectSettings.noDefaultBranch')}
            hint={t('projectSettings.branchHint')}
          />
          {branch ? (
            <button
              type="button"
              className="link-button project-default-clear"
              onClick={() => setBranch(null)}
            >
              {t('projectSettings.clearBranch')}
            </button>
          ) : null}

          {error ? (
            <div className="form-error" role="alert">
              <AlertIcon />
              <span>{error}</span>
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
              ? t('projectSettings.submitting')
              : t('projectSettings.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
