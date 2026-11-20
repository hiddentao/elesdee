export type UploadNoticeKey =
  | 'files.uploadTooLarge'
  | 'files.uploadClosed'
  | 'files.uploadInvalid'
  | 'files.uploadFailed';

/** A message key for an upload failure, or null when nothing should show. */
export function uploadNoticeFor(code: string): UploadNoticeKey | null {
  switch (code) {
    case 'cancelled':
      return null;
    case 'too_large':
      return 'files.uploadTooLarge';
    case 'conflict':
      return 'files.uploadClosed';
    case 'bad_request':
      return 'files.uploadInvalid';
    default:
      return 'files.uploadFailed';
  }
}

/** Builds the image source for the preview from the bridge payload. */
export function imageDataUrl(contentType: string, data: string): string {
  const type = contentType.startsWith('image/')
    ? contentType
    : 'application/octet-stream';
  return `data:${type};base64,${data}`;
}

export type DiffKind = 'added' | 'removed' | 'hunk' | 'context';

export interface DiffRow {
  kind: DiffKind;
  text: string;
}

/** Splits a unified diff into one row per line for the phone view. */
export function diffRows(
  diff: string,
  classify: (line: string) => DiffKind,
): DiffRow[] {
  return diff.split('\n').map((line) => ({
    kind: classify(line),
    text: line || ' ',
  }));
}
