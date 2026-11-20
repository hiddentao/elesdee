import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Provider } from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { UsagePopover } from './UsagePopover';

const provider: Provider = {
  id: 'a1',
  name: 'Anthropic Max 20×',
  vendor: 'anthropic',
  auth: 'subscription',
  server: null,
  sample: false,
  connected: true,
  plan: 'work',
  rows: [
    { label: '5-hour', window: 'five_hour', used: 58 },
    { label: 'Weekly', window: 'seven_day', used: 42 },
  ],
  foot: '',
  supportedClients: ['Claude Code'],
};

describe('UsagePopover', () => {
  it('shows the plan without the vendor slug and names each limit', () => {
    render(
      <LanguageProvider>
        <UsagePopover
          providers={[provider]}
          trigger={null}
          onClose={vi.fn()}
        />
      </LanguageProvider>,
    );

    expect(screen.getByText('work')).toBeInTheDocument();
    expect(screen.queryByText(/anthropic/)).not.toBeInTheDocument();
    expect(screen.getByText('5-hour limit')).toBeInTheDocument();
    expect(screen.getByText('Weekly limit')).toBeInTheDocument();
  });
});
