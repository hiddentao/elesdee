import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { makeSession } from '../test/fixtures';
import { StatusBadge } from './StatusBadge';

describe('StatusBadge', () => {
  it('shows the state without repeating the work detail', () => {
    render(
      <LanguageProvider>
        <StatusBadge
          session={makeSession({ status: 'active', workState: 'working' })}
        />
      </LanguageProvider>,
    );

    const badge = screen.getByText('Active').closest('span.status-badge');
    expect(badge).toHaveTextContent(/^Active$/);
    expect(badge).toHaveAttribute('title', 'Active, Working');
  });

  it('keeps the work detail available for a closed session', () => {
    render(
      <LanguageProvider>
        <StatusBadge session={makeSession({ status: 'closed' })} />
      </LanguageProvider>,
    );

    const badge = screen.getByText('Closed').closest('span.status-badge');
    expect(badge).toHaveTextContent(/^Closed$/);
    expect(badge).toHaveAttribute('title', 'Closed');
  });
});
