import { describe, expect, it } from 'vitest';
import { ApiError } from '../api/transport';
import type { ChangedFile, SessionChanges } from '../api/types';
import {
  changeTotals,
  changesEmpty,
  diffLineKind,
  fileExtension,
  fileSize,
  imageErrorKey,
  isImageName,
  joinFilePath,
  loadFilePreview,
  parentFilePath,
} from './files';

function changed(overrides: Partial<ChangedFile> = {}): ChangedFile {
  return {
    path: 'src/app.ts',
    oldPath: null,
    status: 'modified',
    additions: 2,
    deletions: 1,
    binary: false,
    diff: '',
    diffTruncated: false,
    ...overrides,
  };
}

function changes(overrides: Partial<SessionChanges> = {}): SessionChanges {
  return {
    git: true,
    base: 'abc123',
    files: [changed()],
    truncated: false,
    error: null,
    ...overrides,
  };
}

describe('phone file helpers', () => {
  it('recognises supported image names without case', () => {
    expect(isImageName('shot.png')).toBe(true);
    expect(isImageName('photo.JPEG')).toBe(true);
    expect(isImageName('anim.webp')).toBe(true);
    expect(isImageName('logo.svg')).toBe(false);
    expect(isImageName('notes.md')).toBe(false);
    expect(isImageName('png')).toBe(false);
  });

  it('reads extensions only from the last dot', () => {
    expect(fileExtension('archive.tar.gz')).toBe('gz');
    expect(fileExtension('.env')).toBe('');
    expect(fileExtension('README')).toBe('');
  });

  it('walks a relative path by folder', () => {
    expect(parentFilePath('src/app.ts')).toBe('src');
    expect(parentFilePath('src/ui/button.tsx')).toBe('src/ui');
    expect(parentFilePath('README.md')).toBe('');
    expect(joinFilePath('', 'README.md')).toBe('README.md');
    expect(joinFilePath('src', 'app.ts')).toBe('src/app.ts');
  });

  it('classifies unified diff lines', () => {
    expect(diffLineKind('+added')).toBe('added');
    expect(diffLineKind('-removed')).toBe('removed');
    expect(diffLineKind('+++ b/app.ts')).toBe('context');
    expect(diffLineKind('--- a/app.ts')).toBe('context');
    expect(diffLineKind('@@ -1,2 +1,3 @@')).toBe('hunk');
    expect(diffLineKind(' unchanged')).toBe('context');
  });

  it('sums the change counts across files', () => {
    const totals = changeTotals(
      changes({
        files: [
          changed({ additions: 2, deletions: 1 }),
          changed({ additions: 4, deletions: 0 }),
        ],
      }),
    );
    expect(totals).toEqual({ additions: 6, deletions: 1, files: 2 });
    expect(changeTotals(null)).toEqual({ additions: 0, deletions: 0, files: 0 });
  });

  it('knows when a Git folder has no changes', () => {
    expect(changesEmpty(changes({ files: [] }))).toBe(true);
    expect(changesEmpty(changes())).toBe(false);
    expect(changesEmpty(changes({ files: [], git: false }))).toBe(false);
    expect(changesEmpty(changes({ files: [], error: 'Git failed' }))).toBe(false);
    expect(changesEmpty(null)).toBe(false);
  });

  it('formats a size with a unit', () => {
    expect(fileSize(512)).toEqual({ value: 512, unit: 'B' });
    expect(fileSize(2048)).toEqual({ value: 2, unit: 'KB' });
    expect(fileSize(3 * 1024 * 1024)).toEqual({ value: 3, unit: 'MB' });
  });

  it('maps a failed image read to the size or support message', () => {
    expect(imageErrorKey(new ApiError(413, 'The image is too large.'))).toBe(
      'phone.fileTooLarge',
    );
    expect(imageErrorKey(new ApiError(415, 'Not an image.'))).toBe(
      'phone.fileNotImage',
    );
  });

  it('keeps an image over the cap distinct from an unsupported image', async () => {
    const large = await loadFilePreview('shot.png', 'shot.png', {
      readText: async () => ({ content: '' }),
      readImage: async () => {
        throw new ApiError(413, 'The image is too large.');
      },
    });
    expect(large).toEqual({ kind: 'error', key: 'phone.fileTooLarge' });

    const unsupported = await loadFilePreview('shot.png', 'shot.png', {
      readText: async () => ({ content: '' }),
      readImage: async () => {
        throw new ApiError(415, 'Not an image.');
      },
    });
    expect(unsupported).toEqual({ kind: 'error', key: 'phone.fileNotImage' });
  });

  it('previews an image without an extension after the text read fails', async () => {
    const blob = new Blob(['png'], { type: 'image/png' });
    const preview = await loadFilePreview('SCREENSHOT', 'SCREENSHOT', {
      readText: async () => {
        throw new ApiError(422, 'The file is not UTF-8 text.');
      },
      readImage: async () => blob,
    });
    expect(preview).toEqual({ kind: 'image', blob });
  });

  it('reports a binary file that is not an image as not text', async () => {
    const preview = await loadFilePreview('data.bin', 'data.bin', {
      readText: async () => {
        throw new ApiError(422, 'The file is not UTF-8 text.');
      },
      readImage: async () => {
        throw new ApiError(415, 'Not an image.');
      },
    });
    expect(preview).toEqual({ kind: 'error', key: 'phone.fileNotText' });
  });

  it('reads a text file without an image request', async () => {
    const preview = await loadFilePreview('notes.md', 'notes.md', {
      readText: async () => ({ content: '# Notes' }),
      readImage: async () => {
        throw new Error('the image route must not run');
      },
    });
    expect(preview).toEqual({ kind: 'text', content: '# Notes' });
  });
});
