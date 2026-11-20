import type { SessionChanges } from '../api/types';

/** Image names that the session image route can return. */
const IMAGE_EXTENSIONS = new Set([
  'png',
  'jpg',
  'jpeg',
  'gif',
  'webp',
  'avif',
  'bmp',
]);

/** Returns the lower-case extension of a file name, or an empty string. */
export function fileExtension(name: string): string {
  const dot = name.lastIndexOf('.');
  if (dot <= 0 || dot === name.length - 1) {
    return '';
  }
  return name.slice(dot + 1).toLowerCase();
}

/** Returns true when the name has a supported image extension. */
export function isImageName(name: string): boolean {
  return IMAGE_EXTENSIONS.has(fileExtension(name));
}

/** Returns the folder that holds a relative file path. */
export function parentFilePath(path: string): string {
  const slash = path.lastIndexOf('/');
  return slash === -1 ? '' : path.slice(0, slash);
}

/** Joins a listed folder and an entry name into a relative path. */
export function joinFilePath(folder: string, name: string): string {
  return folder ? `${folder}/${name}` : name;
}

export type DiffLineKind = 'added' | 'removed' | 'hunk' | 'context';

/** Classifies one line of a unified diff for its colour and label. */
export function diffLineKind(line: string): DiffLineKind {
  if (line.startsWith('+') && !line.startsWith('+++')) {
    return 'added';
  }
  if (line.startsWith('-') && !line.startsWith('---')) {
    return 'removed';
  }
  if (line.startsWith('@@')) {
    return 'hunk';
  }
  return 'context';
}

/** Returns the total added and removed lines for a change list. */
export function changeTotals(changes: SessionChanges | null): {
  additions: number;
  deletions: number;
  files: number;
} {
  if (!changes) {
    return { additions: 0, deletions: 0, files: 0 };
  }
  return changes.files.reduce(
    (totals, file) => ({
      additions: totals.additions + file.additions,
      deletions: totals.deletions + file.deletions,
      files: totals.files + 1,
    }),
    { additions: 0, deletions: 0, files: 0 },
  );
}

/** Returns true when a change list has nothing the phone can show. */
export function changesEmpty(changes: SessionChanges | null): boolean {
  return changes !== null && changes.git && changes.files.length === 0 && !changes.error;
}

/** Formats a byte count as a compact size with a unit. */
export function fileSize(bytes: number): { value: number; unit: string } {
  if (bytes < 1024) {
    return { value: bytes, unit: 'B' };
  }
  if (bytes < 1024 * 1024) {
    return { value: Math.round(bytes / 102.4) / 10, unit: 'KB' };
  }
  return { value: Math.round(bytes / (1024 * 1024) * 10) / 10, unit: 'MB' };
}
