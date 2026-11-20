import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import { isTauri, pickFolder } from '../api/shell';
import type { CreateProjectInput, Project } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface AddProjectDialogProps {
  onCreate: (input: CreateProjectInput) => Promise<Project>;
  onCreated: (project: Project) => void;
  onClose: () => void;
}

interface FormErrors {
  name?: string;
  folder?: string;
  form?: string;
}

export function AddProjectDialog({
  onCreate,
  onCreated,
  onClose,
}: AddProjectDialogProps) {
  const { t } = useLanguage();
  const [name, setName] = useState('');
  const [folder, setFolder] = useState('');
  const [errors, setErrors] = useState<FormErrors>({});
  const [submitting, setSubmitting] = useState(false);
  const [focusTarget, setFocusTarget] = useState<string | null>(null);

  useEffect(() => {
    if (!focusTarget) {
      return;
    }

    document.getElementById(focusTarget)?.focus();
    setFocusTarget(null);
  }, [focusTarget]);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const nextErrors: FormErrors = {};
    if (name.trim() === '') {
      nextErrors.name = t('addProject.errorName');
    }
    if (folder.trim() === '') {
      nextErrors.folder = t('addProject.errorFolder');
    }

    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      setFocusTarget(nextErrors.name ? 'add-project-name' : 'add-project-folder');
      return;
    }

    setErrors({});
    setSubmitting(true);
    try {
      const project = await onCreate({
        name: name.trim(),
        folder: folder.trim(),
      });
      onCreated(project);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : t('addProject.errorAdd'),
      });
      setSubmitting(false);
    }
  }

  async function browse() {
    try {
      const chosen = await pickFolder();
      if (chosen) {
        setFolder(chosen);
        setErrors((current) => ({ ...current, folder: undefined }));
      }
    } catch {
      setErrors((current) => ({
        ...current,
        form: t('addProject.errorPicker'),
      }));
    }
  }

  return (
    <Dialog
      titleId="add-project-title"
      descriptionId="add-project-description"
      busyLabel={t('addProject.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="add-project-title">
          {t('addProject.title')}
        </h2>
        <p className="dialog-sub" id="add-project-description">
          {t('addProject.subtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <div className="field">
            <label htmlFor="add-project-name">{t('addProject.name')}</label>
            <input
              id="add-project-name"
              className="text-input"
              value={name}
              placeholder={t('addProject.namePlaceholder')}
              autoFocus
              required
              aria-invalid={errors.name ? true : undefined}
              aria-describedby={
                errors.name ? 'add-project-name-error' : undefined
              }
              onChange={(event) => {
                setName(event.target.value);
                setErrors((current) => ({ ...current, name: undefined }));
              }}
            />
            {errors.name ? (
              <p className="field-error" id="add-project-name-error" role="alert">
                {errors.name}
              </p>
            ) : null}
          </div>

          <div className="field">
            <label htmlFor="add-project-folder">{t('addProject.folder')}</label>
            <div className="field-row">
              <input
                id="add-project-folder"
                className="text-input"
                value={folder}
                placeholder={t('addProject.folderPlaceholder')}
                spellCheck={false}
                required
                aria-invalid={errors.folder ? true : undefined}
                aria-describedby={
                  errors.folder ? 'add-project-folder-error' : undefined
                }
                onChange={(event) => {
                  setFolder(event.target.value);
                  setErrors((current) => ({ ...current, folder: undefined }));
                }}
              />
              {isTauri() ? (
                <button type="button" className="btn btn-ghost" onClick={() => void browse()}>
                  {t('addProject.browse')}
                </button>
              ) : null}
            </div>
            {errors.folder ? (
              <p className="field-error" id="add-project-folder-error" role="alert">
                {errors.folder}
              </p>
            ) : null}
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
            {submitting ? t('addProject.submitting') : t('addProject.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
