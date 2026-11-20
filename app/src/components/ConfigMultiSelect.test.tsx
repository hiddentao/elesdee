import { render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AiConfig } from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { ConfigMultiSelect } from './ConfigMultiSelect';

vi.mock('../api/client', () => ({
  listConfigs: vi.fn(async () => []),
}));

function config(id: string): AiConfig {
  return {
    id,
    name: id,
    providerId: 'prov1',
    client: 'Claude Code',
    model: null,
    effort: null,
    args: [],
    env: [],
    config: {},
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
  };
}

function rect(top: number, bottom: number): DOMRect {
  return {
    top,
    bottom,
    height: bottom - top,
    left: 0,
    right: 100,
    width: 100,
    x: 0,
    y: top,
    toJSON: () => ({}),
  } as DOMRect;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ConfigMultiSelect', () => {
  it('scrolls the current configuration into view', () => {
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
      function mockRect(this: HTMLElement) {
        if (this.classList.contains('allowed-account-list')) {
          return rect(0, 60);
        }
        if (this.dataset.current === 'true') {
          return rect(70, 90);
        }
        return rect(0, 0);
      },
    );

    render(
      <LanguageProvider>
        <ConfigMultiSelect
          id="allowed"
          label="Allowed configurations"
          configs={[config('a'), config('b'), config('c')]}
          providers={[]}
          value={['a', 'b', 'c']}
          requiredId="c"
          onChange={vi.fn()}
        />
      </LanguageProvider>,
    );

    const list = screen.getByRole('list', {
      name: 'Allowed configurations',
    });
    expect(
      list.querySelector('[data-current="true"]'),
    ).toHaveTextContent('c');
    expect(list.scrollTop).toBe(30);
  });
});
