/**
 * The controls a sheet tries when it opens, in priority order.
 *
 * A comma-separated selector returns the first match in document order, so a
 * row action or a checkbox earlier in the sheet would win. Each selector is
 * tried on its own instead.
 */
const PREFERRED_FOCUS = [
  '#switch-filter',
  '#saved-key-name',
  '.sheet-row:not(.is-danger)',
  '.sheet-body button:not([disabled])',
  '.sheet-body input:not([disabled])',
] as const;

/**
 * Returns the control a sheet focuses when it opens.
 *
 * A disabled control and a control inside a hidden or inert region are
 * skipped. When no preferred control is usable, the caller picks the first
 * focusable element.
 */
export function preferredFocusTarget(sheet: HTMLElement): HTMLElement | null {
  for (const selector of PREFERRED_FOCUS) {
    for (const element of sheet.querySelectorAll<HTMLElement>(selector)) {
      if (
        !element.matches(':disabled') &&
        element.closest('[hidden], [inert]') === null &&
        element.getAttribute('aria-hidden') !== 'true'
      ) {
        return element;
      }
    }
  }
  return null;
}
