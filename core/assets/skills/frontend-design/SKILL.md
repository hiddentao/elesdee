---
name: frontend-design
description: Make deliberate visual choices for the subject, avoid generic template defaults, and check for redundant controls before adding one.
---

# Frontend design

## Choose a direction

Look at the subject, the audience and the main job of the screen. Choose a small set of decisions and keep them consistent:

- type: one display face and one text face, or one family with clear weights;
- color: a base, a surface, one accent and clear success, warning and danger colors;
- spacing: one scale, used for every gap and padding;
- shape: one radius or corner style;
- motion: one duration and easing for small transitions.

Write the choices down in the project when it has a design note or tokens.

## Avoid template defaults

These make a screen look generic:

- the browser's default font stack for everything;
- a purple or blue gradient as the only identity;
- a grid of equal cards for content with different weights;
- icons from different families, or emoji as UI icons;
- shadows on every surface, or radius values with no system;
- a centered hero for a tool that needs dense information.

Use them only when the subject calls for them.

## Build the hierarchy

- Use size, weight and spacing before color and decoration.
- Make the main action obvious and keep secondary actions quieter.
- Group related controls. Do not add a control when the same action already exists on the screen.
- Before you add a button, menu item or shortcut, search the current screen and its menus for the same action.

## Make it work at both sizes

- Check the layout at a narrow phone width (about 390 px) and a desktop width (about 1440 px).
- No horizontal page scroll at either width.
- Long names, empty lists and error states must not break the layout.
- Check both light and dark themes when the project has them.

## Accessibility

- Keep text contrast at WCAG AA.
- Every control has a visible focus state and an accessible name.
- Do not use color alone to carry meaning.
- Respect reduced motion for animation.

## Verify

Run the app and look at the changed screen at both widths. Check the console for errors. If the project has browser tests or screenshots, update them.
