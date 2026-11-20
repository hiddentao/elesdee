import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { LanguageProvider } from './i18n/LanguageProvider';
import { FakeEventSource } from './test/fakeEventSource';
import { coreProject, makeSession } from './test/fixtures';

vi.mock('./components/TerminalPane', () => ({
  TerminalPane: () => <div>Terminal pane</div>,
}));

const sessions = [
  makeSession({
    id: 's1',
    name: 'First session',
    configId: null,
    providerId: null,
  }),
  makeSession({
    id: 's2',
    name: 'Second session',
    configId: null,
    providerId: null,
  }),
];

const createdSession = makeSession({
  id: 's3',
  name: 'Created session',
  kind: 'terminal',
  configId: null,
  providerId: null,
  client: null,
});

function payload(data: unknown) {
  return {
    ok: true,
    status: 200,
    json: async () => data,
  };
}

beforeEach(() => {
  FakeEventSource.reset();
  vi.stubGlobal('EventSource', FakeEventSource);
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (
        url.endsWith('/api/sessions') &&
        init?.method === 'POST'
      ) {
        return payload(createdSession);
      }
      if (url.includes('/api/projects?')) {
        return payload([coreProject]);
      }
      if (url.endsWith('/api/bootstrap')) {
        return payload({
          projects: [coreProject],
          providers: [],
          configs: [],
          sessions,
          questions: [],
          sessionRequests: [],
          settings: { startAtLogin: false, keepAwake: false },
        });
      }
      if (url.endsWith('/api/health')) {
        return payload({ ok: true, version: '0.1.0', agents: [] });
      }
      if (url.endsWith('/api/provider-options')) {
        return payload([]);
      }
      return payload({});
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('App window lifecycle', () => {
  it('shows the same open tab after the window hides and shows again', async () => {
    const user = userEvent.setup();
    const { container } = render(
      <LanguageProvider>
        <App />
      </LanguageProvider>,
    );

    await user.click(
      await screen.findByRole('button', { name: /First session/ }),
    );
    expect(
      await screen.findByRole('tab', { name: /First session/ }),
    ).toHaveAttribute('aria-selected', 'true');

    // The native window hides and shows without reloading the web view.
    const shell = container.firstElementChild as HTMLElement;
    shell.style.display = 'none';
    fireEvent(window, new Event('blur'));
    shell.style.display = '';
    fireEvent(window, new Event('focus'));

    const shownTab = screen.getByRole('tab', { name: /First session/ });
    expect(shownTab).toHaveAttribute('aria-selected', 'true');
    expect(screen.getAllByRole('tab')).toHaveLength(1);
  });

  it('opens the side panel without duplicate sibling keys', async () => {
    const user = userEvent.setup();
    const errors: string[] = [];
    const spy = vi.spyOn(console, 'error').mockImplementation((...args) => {
      errors.push(args.map(String).join(' '));
    });
    try {
      render(
        <LanguageProvider>
          <App />
        </LanguageProvider>,
      );

      await user.click(
        await screen.findByRole('button', { name: /First session/ }),
      );
      spy.mockClear();

      await user.click(
        await screen.findByRole('button', { name: 'Session panel' }),
      );
      expect(
        await screen.findByRole('tab', { name: 'Files' }),
      ).toBeInTheDocument();

      expect(
        errors.filter((message) => message.includes('same key')),
      ).toEqual([]);
    } finally {
      spy.mockRestore();
    }
  });

  it('starts the heading order at level one and keeps it in order', async () => {
    const user = userEvent.setup();
    render(
      <LanguageProvider>
        <App />
      </LanguageProvider>,
    );

    await user.click(
      await screen.findByRole('button', { name: /First session/ }),
    );

    expect(
      screen.getByRole('heading', { level: 1, name: 'First session' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { level: 1, name: /elesdee/ }),
    ).toBeInTheDocument();

    const levels = screen
      .getAllByRole('heading')
      .map((heading) => Number(heading.tagName.slice(1)));
    expect(levels[0]).toBe(1);
    for (let index = 1; index < levels.length; index += 1) {
      expect(levels[index] - levels[index - 1]).toBeLessThanOrEqual(1);
    }
  });

  it('moves focus to the new session tab after creation', async () => {
    const user = userEvent.setup();
    render(
      <LanguageProvider>
        <App />
      </LanguageProvider>,
    );

    await waitFor(() =>
      expect(document.querySelector('.titlebar')).not.toBeNull(),
    );
    const titlebar = document.querySelector('.titlebar') as HTMLElement;
    await user.click(
      within(titlebar).getByRole('button', { name: 'New session' }),
    );
    const dialog = await screen.findByRole('dialog');

    await user.click(
      within(dialog).getByRole('radio', { name: 'Terminal' }),
    );
    await user.click(
      within(dialog).getByRole('button', { name: 'Start session' }),
    );

    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(
      await screen.findByRole('tab', { name: /Created session/ }),
    ).toHaveFocus();
  });
});
