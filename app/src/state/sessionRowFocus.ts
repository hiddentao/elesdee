/**
 * Focus the sidebar row for a session. When the row is missing, focus the
 * fallback element named by a selector, if one is given.
 */
export function focusSessionRow(
  sessionId: string,
  fallbackSelector?: string,
): void {
  const rows = document.querySelectorAll<HTMLElement>('[data-session-id]');
  for (const row of rows) {
    if (row.dataset.sessionId === sessionId) {
      row.focus();
      return;
    }
  }
  if (fallbackSelector) {
    document.querySelector<HTMLElement>(fallbackSelector)?.focus();
  }
}
