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

type PreviewNoticeKey =
  | 'files.imageTooLarge'
  | 'files.notImage'
  | 'files.notText';

function errorCode(error: unknown): string | null {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  ) {
    return (error as { code: string }).code;
  }
  return null;
}

function errorStatus(error: unknown): number | null {
  if (
    typeof error === 'object' &&
    error !== null &&
    'status' in error &&
    typeof (error as { status: unknown }).status === 'number'
  ) {
    return (error as { status: number }).status;
  }
  return null;
}

/** Returns the message key for a failed image read. */
export function imageNoticeFor(error: unknown): PreviewNoticeKey {
  return errorCode(error) === 'too_large' || errorStatus(error) === 413
    ? 'files.imageTooLarge'
    : 'files.notImage';
}

/** Returns true when the text read failed because the file is binary. */
function isBinaryError(error: unknown): boolean {
  return errorCode(error) === 'unprocessable' || errorStatus(error) === 422;
}

export type PreviewResult<TText, TImage> =
  | { kind: 'image'; value: TImage }
  | { kind: 'text'; value: TText }
  | { kind: 'error'; key: PreviewNoticeKey };

/**
 * Loads a file preview for a chosen entry.
 *
 * A name with an image extension goes straight to the image read. Any other
 * name tries text first, then the image read when the text read reports binary
 * data, so an image without an extension still previews.
 */
export async function loadPreview<TText, TImage>(
  imageName: boolean,
  readText: () => Promise<TText>,
  readImage: () => Promise<TImage>,
): Promise<PreviewResult<TText, TImage>> {
  if (imageName) {
    try {
      return { kind: 'image', value: await readImage() };
    } catch (error) {
      return { kind: 'error', key: imageNoticeFor(error) };
    }
  }
  try {
    return { kind: 'text', value: await readText() };
  } catch (error) {
    if (!isBinaryError(error)) {
      return { kind: 'error', key: 'files.notText' };
    }
    try {
      return { kind: 'image', value: await readImage() };
    } catch (imageError) {
      return {
        kind: 'error',
        key:
          imageNoticeFor(imageError) === 'files.imageTooLarge'
            ? 'files.imageTooLarge'
            : 'files.notText',
      };
    }
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
