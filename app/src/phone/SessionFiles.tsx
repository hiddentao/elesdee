import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ApiError,
  listFiles,
  readFile,
  readSessionImage,
  sessionChanges,
  uploadFile,
} from '../api/client';
import type {
  FileList,
  Session,
  SessionChanges,
} from '../api/types';
import {
  formatNumber,
  type Language,
  type MessageKey,
  type Translate,
} from '../i18n';
import {
  changeTotals,
  changesEmpty,
  diffLineKind,
  isImageName,
  parentFilePath,
} from './files';

export type PhoneFilePanel = 'files' | 'changes';

type Preview =
  | { kind: 'image'; name: string; url: string }
  | { kind: 'text'; name: string; content: string };

interface Props {
  session: Session;
  panel: PhoneFilePanel;
  t: Translate;
  language: Language;
}

function uploadErrorKey(error: unknown): MessageKey {
  if (error instanceof ApiError) {
    if (error.status === 413) {
      return 'phone.uploadTooLarge';
    }
    if (error.status === 409) {
      return 'phone.uploadClosed';
    }
    if (error.status === 400 || error.status === 415) {
      return 'phone.uploadInvalid';
    }
  }
  return 'phone.uploadFailed';
}

/** The session's files and changes for the phone layout. */
export function SessionFiles({ session, panel, t, language }: Props) {
  const [filesPath, setFilesPath] = useState('');
  const [list, setList] = useState<FileList | null>(null);
  const [filesLoading, setFilesLoading] = useState(false);
  const [filesError, setFilesError] = useState('');
  const [preview, setPreview] = useState<Preview | null>(null);
  const [changes, setChanges] = useState<SessionChanges | null>(null);
  const [changesLoading, setChangesLoading] = useState(false);
  const [changesError, setChangesError] = useState('');
  const [openDiff, setOpenDiff] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);
  const [uploadNotice, setUploadNotice] = useState('');
  const [uploadError, setUploadError] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const previewUrl = useRef<string | null>(null);
  const tRef = useRef(t);
  tRef.current = t;

  const releasePreview = useCallback(() => {
    if (previewUrl.current) {
      URL.revokeObjectURL(previewUrl.current);
      previewUrl.current = null;
    }
    setPreview(null);
  }, []);

  useEffect(() => {
    releasePreview();
    setFilesPath('');
    setList(null);
    setFilesError('');
    setChanges(null);
    setChangesError('');
    setOpenDiff(null);
    setUploadNotice('');
    setUploadError('');
  }, [session.id, releasePreview]);

  useEffect(
    () => () => {
      if (previewUrl.current) {
        URL.revokeObjectURL(previewUrl.current);
        previewUrl.current = null;
      }
    },
    [],
  );

  const loadFiles = useCallback(
    async (path: string) => {
      setFilesLoading(true);
      setFilesError('');
      try {
        const next = await listFiles(session.id, path);
        setList(next);
        setFilesPath(path);
      } catch {
        setFilesError(tRef.current('phone.filesFailed'));
      } finally {
        setFilesLoading(false);
      }
    },
    [session.id],
  );

  const loadChanges = useCallback(async () => {
    setChangesLoading(true);
    setChangesError('');
    try {
      setChanges(await sessionChanges(session.id));
    } catch {
      setChangesError(tRef.current('phone.changesFailed'));
    } finally {
      setChangesLoading(false);
    }
  }, [session.id]);

  useEffect(() => {
    if (panel === 'files' && list === null && !filesLoading && !filesError) {
      void loadFiles('');
    }
    if (panel === 'changes' && changes === null && !changesLoading && !changesError) {
      void loadChanges();
    }
  }, [panel, list, filesLoading, filesError, changes, changesLoading, changesError, loadFiles, loadChanges]);

  const openFile = useCallback(
    async (path: string, name: string, directory: boolean) => {
      if (directory) {
        void loadFiles(path);
        return;
      }
      setFilesError('');
      releasePreview();
      if (isImageName(name)) {
        try {
          const blob = await readSessionImage(session.id, path);
          const url = URL.createObjectURL(blob);
          previewUrl.current = url;
          setPreview({ kind: 'image', name: path, url });
        } catch {
          setFilesError(tRef.current('phone.fileNotImage'));
        }
        return;
      }
      try {
        const file = await readFile(session.id, path);
        setPreview({ kind: 'text', name: path, content: file.content });
      } catch {
        setFilesError(tRef.current('phone.fileNotText'));
      }
    },
    [session.id, loadFiles, releasePreview],
  );

  const pickUpload = useCallback(() => {
    setUploadNotice('');
    setUploadError('');
    inputRef.current?.click();
  }, []);

  const uploadPicked = useCallback(
    async (file: File | undefined) => {
      if (!file) {
        return;
      }
      setUploading(true);
      setUploadNotice('');
      setUploadError('');
      try {
        const uploaded = await uploadFile(session.id, file.name, file);
        setUploadNotice(
          tRef.current('phone.uploadDone', { name: uploaded.name }),
        );
      } catch (error) {
        setUploadError(tRef.current(uploadErrorKey(error)));
      } finally {
        setUploading(false);
      }
    },
    [session.id],
  );

  const totals = changeTotals(changes);

  return (
    <section
      className="phone-session-files"
      aria-label={t(panel === 'files' ? 'phone.files' : 'phone.changes')}
    >
      {panel === 'files' ? (
        <>
          <div className="phone-panel-head">
            <h2 className="phone-section-title">{t('phone.files')}</h2>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={pickUpload}
              disabled={uploading || session.status === 'closed'}
            >
              {uploading ? t('phone.uploading') : t('phone.upload')}
            </button>
            <input
              ref={inputRef}
              className="phone-file-input"
              type="file"
              aria-label={t('phone.upload')}
              onChange={(event) => {
                const file = event.target.files?.[0];
                event.target.value = '';
                void uploadPicked(file);
              }}
            />
          </div>
          {uploadNotice ? (
            <p className="phone-upload-notice" role="status">
              {uploadNotice}
            </p>
          ) : null}
          {uploadError ? (
            <p className="phone-card-error" role="alert">
              {uploadError}
            </p>
          ) : null}
          <div className="phone-file-path">
            <span>{filesPath ? `/${filesPath}` : '/'}</span>
            <div className="phone-file-actions">
              {filesPath ? (
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() => void loadFiles(parentFilePath(filesPath))}
                >
                  {t('phone.folderUp')}
                </button>
              ) : null}
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => void loadFiles(filesPath)}
              >
                {t('phone.filesRefresh')}
              </button>
            </div>
          </div>
          {filesLoading ? (
            <p className="phone-note" role="status" aria-busy="true">
              {t('phone.filesLoading')}
            </p>
          ) : filesError ? (
            <p className="phone-card-error" role="alert">
              {filesError}
            </p>
          ) : list === null || list.entries.length === 0 ? (
            <p className="phone-note">{t('phone.filesEmpty')}</p>
          ) : (
            <ul className="phone-file-list">
              {list.entries.map((entry) => (
                <li key={entry.path}>
                  <button
                    type="button"
                    className="phone-file-row"
                    onClick={() => void openFile(entry.path, entry.name, entry.directory)}
                  >
                    <span className="phone-file-name">{entry.name}</span>
                    <span className="phone-file-kind">
                      {entry.directory
                        ? t('phone.folder')
                        : isImageName(entry.name)
                          ? t('phone.image')
                          : ''}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {list?.truncated ? (
            <p className="phone-note">{t('phone.filesTruncated')}</p>
          ) : null}
          {preview ? (
            <div className="phone-file-preview">
              <div className="phone-panel-head">
                <span className="phone-file-preview-name">{preview.name}</span>
                <button type="button" className="btn btn-ghost" onClick={releasePreview}>
                  {t('common.close')}
                </button>
              </div>
              {preview.kind === 'image' ? (
                <img
                  className="phone-image"
                  src={preview.url}
                  alt={t('phone.imagePreview', { name: preview.name })}
                />
              ) : (
                <pre className="phone-file-text">{preview.content}</pre>
              )}
            </div>
          ) : null}
        </>
      ) : (
        <>
          <div className="phone-panel-head">
            <h2 className="phone-section-title">{t('phone.changes')}</h2>
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => void loadChanges()}
            >
              {t('phone.filesRefresh')}
            </button>
          </div>
          {changesLoading ? (
            <p className="phone-note" role="status" aria-busy="true">
              {t('phone.changesLoading')}
            </p>
          ) : changesError ? (
            <p className="phone-card-error" role="alert">
              {changesError}
            </p>
          ) : changes === null ? null : !changes.git ? (
            <p className="phone-note">{t('phone.changesNotGit')}</p>
          ) : (
            <>
              <p className="phone-change-summary">
                {t('phone.changesSummary', {
                  files: formatNumber(language, totals.files),
                  additions: formatNumber(language, totals.additions),
                  deletions: formatNumber(language, totals.deletions),
                })}
              </p>
              {changes.error ? (
                <p className="phone-card-error" role="alert">
                  {changes.error}
                </p>
              ) : null}
              {changesEmpty(changes) ? (
                <p className="phone-note">{t('phone.changesEmpty')}</p>
              ) : (
                <ul className="phone-change-list">
                  {changes.files.map((file) => (
                    <li key={file.path}>
                      <button
                        type="button"
                        className="phone-change-row"
                        aria-expanded={openDiff === file.path}
                        aria-label={t('phone.changeOpen', { path: file.path })}
                        onClick={() =>
                          setOpenDiff((current) =>
                            current === file.path ? null : file.path,
                          )
                        }
                      >
                        <span className="phone-change-path">{file.path}</span>
                        <span className="phone-change-stat">
                          <b>+{formatNumber(language, file.additions)}</b>{' '}
                          <i>−{formatNumber(language, file.deletions)}</i>
                        </span>
                      </button>
                      {openDiff === file.path ? (
                        file.binary ? (
                          <p className="phone-note">{t('phone.changeBinary')}</p>
                        ) : (
                          <pre className="phone-diff">
                            {file.diff.split('\n').map((line, index) => (
                              <span
                                key={index}
                                className={`phone-diff-line is-${diffLineKind(line)}`}
                              >
                                {line || ' '}
                              </span>
                            ))}
                          </pre>
                        )
                      ) : null}
                      {openDiff === file.path && file.diffTruncated ? (
                        <p className="phone-note">{t('phone.changeTruncated')}</p>
                      ) : null}
                    </li>
                  ))}
                </ul>
              )}
              {changes.truncated ? (
                <p className="phone-note">{t('phone.changesTruncated')}</p>
              ) : null}
            </>
          )}
        </>
      )}
    </section>
  );
}
