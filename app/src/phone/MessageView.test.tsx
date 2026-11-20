import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { HistoryItem, Session } from '../api/types';
import { translate, type Translate } from '../i18n';
import { makeSession } from '../test/fixtures';
import { MessageView, SessionContextStrip } from './MessageView';

const t: Translate = (key, values) => translate('en', key, values);

function session(overrides: Partial<Session> = {}): Session {
  return makeSession({ contextLeft: 42, ...overrides });
}

function item(overrides: Partial<HistoryItem> = {}): HistoryItem {
  return {
    seq: 1,
    id: 'item-1',
    kind: 'reply',
    text: 'Fixed the test.',
    state: 'done',
    ...overrides,
  };
}

describe('MessageView', () => {
  it('shows prompts and replies in full with one-line tool rows', () => {
    render(
      <MessageView
        session={session()}
        items={[
          item({ id: 'prompt', kind: 'prompt', text: 'Fix the failing test' }),
          item({ id: 'reply', kind: 'reply', text: 'Looking at it now.\nDone.' }),
          item({
            seq: 3,
            id: 'tool',
            kind: 'tool',
            tool: 'Bash',
            text: 'cargo test',
            state: 'running',
          }),
        ]}
        loading={false}
        expanded={new Set()}
        onToggleTool={vi.fn()}
        t={t}
      />,
    );

    expect(screen.getByText('You')).toBeInTheDocument();
    expect(screen.getByText('Fix the failing test')).toBeInTheDocument();
    expect(screen.getByText(/Looking at it now/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Bash/ })).toBeInTheDocument();
    expect(screen.getByText('Running')).toBeInTheDocument();
    expect(screen.queryByText('cargo test', { selector: 'pre' })).toBeNull();
  });

  it('opens a tool row on tap and closes it again', async () => {
    const user = userEvent.setup();
    const onToggleTool = vi.fn();
    const tool = item({
      id: 'tool',
      kind: 'tool',
      tool: 'Bash',
      text: 'cargo test --all',
      state: 'done',
    });

    const { rerender } = render(
      <MessageView
        session={session()}
        items={[tool]}
        loading={false}
        expanded={new Set()}
        onToggleTool={onToggleTool}
        t={t}
      />,
    );

    await user.click(screen.getByRole('button', { name: /Bash/ }));
    expect(onToggleTool).toHaveBeenCalledWith('tool');

    rerender(
      <MessageView
        session={session()}
        items={[tool]}
        loading={false}
        expanded={new Set(['tool'])}
        onToggleTool={onToggleTool}
        t={t}
      />,
    );
    expect(screen.getByText('cargo test --all', { selector: 'pre' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Bash/ })).toHaveAttribute(
      'aria-expanded',
      'true',
    );
  });

  it('shows the loading and empty states', () => {
    const { rerender } = render(
      <MessageView
        session={session()}
        items={[]}
        loading
        expanded={new Set()}
        onToggleTool={vi.fn()}
        t={t}
      />,
    );
    expect(screen.getByText('Loading messages…')).toBeInTheDocument();

    rerender(
      <MessageView
        session={session()}
        items={[]}
        loading={false}
        expanded={new Set()}
        onToggleTool={vi.fn()}
        t={t}
      />,
    );
    expect(screen.getByText('No messages yet.')).toBeInTheDocument();
  });

  it('shows the done state after the last item', () => {
    render(
      <MessageView
        session={session({ workState: 'done' })}
        items={[item()]}
        loading={false}
        expanded={new Set()}
        onToggleTool={vi.fn()}
        t={t}
      />,
    );
    expect(screen.getByText('Finished')).toBeInTheDocument();
  });
});

describe('SessionContextStrip', () => {
  it('shows the context left and switches to the terminal', async () => {
    const user = userEvent.setup();
    const onToggleView = vi.fn();
    render(
      <SessionContextStrip
        session={session({ branch: 'elesdee/web/fix', workState: 'working' })}
        view="messages"
        onToggleView={onToggleView}
        t={t}
        language="en"
      />,
    );

    expect(screen.getByText('Working')).toBeInTheDocument();
    expect(screen.getByText('42% context left')).toBeInTheDocument();
    expect(screen.getByText('elesdee/web/fix')).toBeInTheDocument();

    const toggle = screen.getByRole('button', { name: 'Session view' });
    expect(toggle).toHaveTextContent('Terminal');
    await user.click(toggle);
    expect(onToggleView).toHaveBeenCalledTimes(1);
  });

  it('says when the context figure is unknown', () => {
    render(
      <SessionContextStrip
        session={session({ contextLeft: null })}
        view="terminal"
        onToggleView={vi.fn()}
        t={t}
        language="en"
      />,
    );
    expect(screen.getByText('Context use unknown')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Session view' })).toHaveTextContent(
      'Messages',
    );
  });
});
