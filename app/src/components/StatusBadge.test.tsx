import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { makeSession } from '../test/fixtures';
import { StatusBadge } from './StatusBadge';

function badge(session: Parameters<typeof StatusBadge>[0]['session']) {
  render(
    <LanguageProvider>
      <StatusBadge session={session} />
    </LanguageProvider>,
  );
  return screen.getByText(/Active|Closed/).closest('span.status-badge');
}

describe('StatusBadge', () => {
  it('shows the work state as words and a shape', () => {
    const element = badge(
      makeSession({ status: 'active', workState: 'working' }),
    );

    expect(element).toHaveTextContent('Active, Working');
    expect(
      element?.querySelector('.status-marker'),
    ).toHaveAttribute('data-shape', 'dot');
  });

  it('tells a paused session apart from one that waits for input', () => {
    const paused = makeSession({ status: 'active', workState: 'paused' });
    const { unmount } = render(
      <LanguageProvider>
        <StatusBadge session={paused} />
      </LanguageProvider>,
    );
    expect(screen.getByText('Active, Paused')).toBeInTheDocument();
    expect(screen.getByText('Active, Paused').closest('.status-badge'))
      .toHaveTextContent('Active, Paused');
    expect(document.querySelector('.status-marker')).toHaveAttribute(
      'data-shape',
      'square-ring',
    );
    unmount();

    render(
      <LanguageProvider>
        <StatusBadge
          session={makeSession({ status: 'active', workState: 'input' })}
        />
      </LanguageProvider>,
    );
    expect(screen.getByText('Active, Waiting for you')).toBeInTheDocument();
    expect(document.querySelector('.status-marker')).toHaveAttribute(
      'data-shape',
      'square',
    );
  });

  it('shows a closed session without a work state', () => {
    const element = badge(makeSession({ status: 'closed' }));

    expect(element).toHaveTextContent(/^Closed$/);
    expect(element?.querySelector('.status-marker')).toHaveAttribute(
      'data-shape',
      'hollow',
    );
  });
});
