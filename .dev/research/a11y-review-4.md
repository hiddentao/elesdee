# Accessibility review 4: desktop and phone

Target: app code at `8f24897` (the app code at `e091ea1` is unchanged). Reviewed at
1440x900 and 1024x800 on the desktop and 390x844 on the phone, light and dark,
with Chrome 158 and its accessibility tree. No screen reader was run.

Result: 6 high, 8 medium, 4 low. No blocker. Every surface can be reached, but
several dialogs and phone sheets open with focus outside them, and the side
panel has two misleading ARIA patterns.

## Ranked findings

| # | Severity | Screen | Finding | WCAG |
| --- | --- | --- | --- | --- |
| 1 | High | Settings, phone | Dialogs open with focus outside them; Escape fails until Tab enters | 2.4.3, 2.1.1, 4.1.2 |
| 2 | High | Phone | Tab, start, keys and switch sheets are dialogs without focus, trap, Escape or modal semantics | 2.4.3, 2.1.1, 4.1.2 |
| 3 | High | Side panel | Dock and browser tablists use tab roles without arrow keys or roving tabindex | 2.1.1, 4.1.2 |
| 4 | High | Side panel | File tree declares a tree but has no tree keyboard model | 2.1.1, 4.1.2, 2.4.3 |
| 5 | High | Side panel | Code editor textbox has no accessible name | 4.1.2, 1.3.1 |
| 6 | High | Sidebar | Moving a session has no single-pointer alternative | 2.5.7 |
| 7 | Medium | Panels, phone | Focus goes to BODY after closing the dock, a phone sheet or a phone tab | 2.4.3 |
| 8 | Medium | Launch form | Busy Tab leaves the dialog; created session focus lands on New session | 2.4.3, 4.1.3 |
| 9 | Medium | Sidebar/tabs | Work state is colour-only for sighted users | 1.4.1 |
| 10 | Medium | Editor/diff | Code comments, line numbers and diff metadata fail text contrast | 1.4.3 |
| 11 | Medium | Phone | Permission, question and request cards have no live region | 4.1.3 |
| 12 | Medium | Phone launch | Validation error does not move focus to the first invalid field | 3.3.1, 3.3.3 |
| 13 | Medium | Terminal | Terminal output is an unlabelled list with an assertive live region | 1.3.1, 4.1.3 |
| 14 | Medium | Side panel | Code editor focus is shown only by the text caret | 2.4.7 |
| 15 | Low | Desktop/phone | Heading order starts at H2; phone screen titles are spans | 1.3.1, 2.4.6 |
| 16 | Low | Phone | Active phone tab has no programmatic current state | 1.3.1, 4.1.2 |
| 17 | Low | Header/phone | Panel toggle and tab-sheet trigger lack `aria-controls` | 4.1.2 |
| 18 | Low | Side panel | File rows are 23px high and adjacent | 2.5.8 |

## High findings

### 1. Dialogs do not take initial focus

The shared dialog root is `app/src/components/Dialog.tsx`. Settings > AI configs >
Add AI config and Remove provider open with `document.activeElement` on `BODY`;
Escape does nothing until Tab enters the dialog. The phone Usage and Close session
dialogs leave focus on the button behind them, and forty Tabs do not reach the
Usage dialog. Expected: focus moves into the modal, Tab stays inside, Escape
closes, and focus returns to the trigger.

### 2. Phone sheets are not modal

The phone tab, start, keys and switch sheets use dialog markup but do not move
focus, trap Tab, handle Escape or mark the rest of the page inert.

### 3. Side-panel tablists lack the keyboard model

Dock and browser tabs use tab roles with neither arrow keys nor a roving
`tabindex`, so every tab is a separate stop and the arrow keys do nothing.

### 4. File tree lacks the tree keyboard model

`app/src/components` file rows declare a tree without arrow-key movement,
expanded-state announcement or a single roving tab stop. Every file is a stop.

### 5. Code editor has no accessible name

The CodeMirror content textbox has no label, so a screen reader announces an
unnamed edit field.

### 6. No single-pointer way to move a session

Moving a session is drag or the `M` key only. WCAG 2.5.7 needs a simple pointer
action, such as a menu item, in addition to dragging.

## Medium and low findings

- Focus returns to `BODY` rather than the trigger after closing the dock, a
  phone sheet or a phone tab.
- The launch form lets Tab leave while busy, and focus lands on New session
  rather than the new tab.
- Sidebar and tab work state is colour-only; only Active and Closed have text.
- Code comments, line numbers and diff metadata fail text contrast.
- Phone permission, question and request cards have no live region.
- The phone launch validation error does not focus the first invalid field.
- Terminal output is an unlabelled list inside an assertive live region.
- The code editor has no visible focus ring beyond the caret.
- Headings start at H2; phone screen and session titles are spans.
- The active phone tab has no programmatic current state.
- The panel toggle and tab-sheet trigger lack `aria-controls`.
- File rows are 23px high and adjacent, below the 24px target.

## Suggested order

Fix the shared dialog root and the phone sheet base first, then the side-panel
tab and tree keyboard models, the editor label and focus ring, and the
single-pointer move action. The medium contrast, live-region and focus-return
items follow in the same components.
