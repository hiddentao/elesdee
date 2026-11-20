import { StrictMode, useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { Dialog } from './Dialog';

function Harness() {
  const [open, setOpen] = useState(false);

  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open dialog
      </button>
      {open ? (
        <Dialog
          titleId="test-title"
          descriptionId="test-description"
          onClose={() => setOpen(false)}
        >
          <h2 id="test-title">Test dialog</h2>
          <p id="test-description">A description</p>
          <button type="button" onClick={() => setOpen(false)} autoFocus>
            Close
          </button>
        </Dialog>
      ) : null}
    </>
  );
}

function TrapHarness({ autoFocusField = false }: { autoFocusField?: boolean }) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open dialog
      </button>
      {open ? (
        <Dialog titleId="trap-title" onClose={() => setOpen(false)}>
          <h2 id="trap-title">Trap dialog</h2>
          <button type="button">First</button>
          {autoFocusField ? (
            <input aria-label="Name" autoFocus />
          ) : (
            <button type="button">Second</button>
          )}
        </Dialog>
      ) : null}
    </>
  );
}

describe('Dialog', () => {
  it('moves focus to the first control on open', async () => {
    const user = userEvent.setup();
    render(<TrapHarness />);

    await user.click(screen.getByRole('button', { name: 'Open dialog' }));

    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();
  });

  it('keeps focus inside while StrictMode repeats the mount effects', async () => {
    const user = userEvent.setup();
    render(
      <StrictMode>
        <TrapHarness />
      </StrictMode>,
    );

    await user.click(screen.getByRole('button', { name: 'Open dialog' }));

    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();
  });

  it('keeps focus on a field that asks for it on open', async () => {
    const user = userEvent.setup();
    render(<TrapHarness autoFocusField />);

    await user.click(screen.getByRole('button', { name: 'Open dialog' }));

    expect(screen.getByLabelText('Name')).toHaveFocus();
    expect(screen.getByRole('button', { name: 'First' })).not.toHaveFocus();
  });

  it('focuses the dialog itself when it has no controls', () => {
    render(
      <Dialog titleId="empty-title" onClose={() => {}}>
        <h2 id="empty-title">Empty dialog</h2>
      </Dialog>,
    );

    expect(screen.getByRole('dialog')).toHaveFocus();
  });

  it('traps Tab and Shift+Tab inside the dialog', async () => {
    const user = userEvent.setup();
    render(<TrapHarness />);

    await user.click(screen.getByRole('button', { name: 'Open dialog' }));
    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();

    await user.tab();
    expect(screen.getByRole('button', { name: 'Second' })).toHaveFocus();

    await user.tab();
    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();

    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Second' })).toHaveFocus();
  });

  it('links its description and restores focus to the opener', async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const opener = screen.getByRole('button', { name: 'Open dialog' });

    await user.click(opener);

    expect(screen.getByRole('dialog')).toHaveAttribute(
      'aria-describedby',
      'test-description',
    );

    await user.click(screen.getByRole('button', { name: 'Close' }));

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it('marks a busy dialog and focuses its status', () => {
    render(
      <Dialog
        titleId="test-title"
        busy
        busyLabel="Starting session…"
        onClose={() => {}}
      >
        <h2 id="test-title">Busy dialog</h2>
      </Dialog>,
    );

    expect(screen.getByRole('dialog')).toHaveAttribute('aria-busy', 'true');
    expect(screen.getByRole('status')).toHaveTextContent('Starting session…');
    expect(screen.getByRole('status')).toHaveFocus();
  });

  it('keeps Tab inside the dialog while busy', async () => {
    const user = userEvent.setup();
    render(
      <Dialog
        titleId="busy-title"
        busy
        busyLabel="Starting session…"
        onClose={() => {}}
      >
        <h2 id="busy-title">Busy dialog</h2>
        <button type="button">First</button>
        <button type="button">Second</button>
      </Dialog>,
    );

    expect(screen.getByRole('status')).toHaveFocus();

    await user.tab();
    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();

    await user.tab();
    expect(screen.getByRole('button', { name: 'Second' })).toHaveFocus();

    await user.tab();
    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();

    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Second' })).toHaveFocus();
  });

  it('puts focus back inside when a busy request ends', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const renderDialog = (busy: boolean) => (
      <Dialog
        titleId="busy-end-title"
        busy={busy}
        busyLabel="Starting session…"
        onClose={onClose}
      >
        <h2 id="busy-end-title">Busy dialog</h2>
        <button type="button">First</button>
        <button type="button">Second</button>
      </Dialog>
    );
    const view = render(renderDialog(true));
    expect(screen.getByRole('status')).toHaveFocus();

    view.rerender(renderDialog(false));

    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();

    await user.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('restores focus to the opener after a StrictMode mount', async () => {
    const user = userEvent.setup();
    render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );
    const opener = screen.getByRole('button', { name: 'Open dialog' });

    await user.click(opener);
    await user.keyboard('{Escape}');

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it('restores focus to the opener when Escape closes it', async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const opener = screen.getByRole('button', { name: 'Open dialog' });

    await user.click(opener);
    await user.keyboard('{Escape}');

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });
});
