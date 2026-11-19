import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
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

describe('Dialog', () => {
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
