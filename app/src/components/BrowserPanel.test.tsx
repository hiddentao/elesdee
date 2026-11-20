import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  activateBrowserTab,
  browserBack,
  browserForward,
  browserReload,
  browserScreenshotUrl,
  closeBrowserTab,
  createBrowserTab,
  getBrowserState,
  openBrowserUrl,
  setupBrowser,
} from '../api/client';
import type { BrowserState, BrowserTab, Session } from '../api/types';
import { makeSession } from '../test/fixtures';
import { BrowserPanel } from './BrowserPanel';

vi.mock('../api/client', () => ({
  activateBrowserTab: vi.fn(),
  browserBack: vi.fn(),
  browserForward: vi.fn(),
  browserReload: vi.fn(),
  browserScreenshotUrl: vi.fn(),
  closeBrowserTab: vi.fn(),
  createBrowserTab: vi.fn(),
  getBrowserState: vi.fn(),
  openBrowserUrl: vi.fn(),
  setupBrowser: vi.fn(),
}));

function emptyState(sessionId = 's1'): BrowserState {
  return {
    sessionId,
    started: false,
    available: true,
    tabs: [],
    activeTabId: null,
    screenshotSeq: 0,
    error: null,
    setup: {
      phase: 'idle',
      downloaded: 0,
      total: null,
      version: null,
      source: null,
      error: null,
    },
  };
}

function tab(overrides: Partial<BrowserTab> = {}): BrowserTab {
  return {
    id: 's1-tab-1',
    url: 'http://localhost:3000/',
    title: 'Demo',
    loading: false,
    canGoBack: false,
    canGoForward: false,
    ...overrides,
  };
}

function stateWith(
  tabs: BrowserTab[],
  overrides: Partial<BrowserState> = {},
): BrowserState {
  return {
    ...emptyState(),
    started: true,
    tabs,
    activeTabId: tabs[0]?.id ?? null,
    screenshotSeq: 1,
    ...overrides,
  };
}

beforeEach(() => {
  vi.mocked(getBrowserState).mockResolvedValue(emptyState());
  vi.mocked(browserScreenshotUrl).mockImplementation(
    (id, tabId, seq) =>
      `/api/sessions/${id}/browser/screenshot?tab=${tabId}&v=${seq}`,
  );
  vi.mocked(openBrowserUrl).mockResolvedValue(stateWith([tab()]));
  vi.mocked(activateBrowserTab).mockResolvedValue(stateWith([tab()]));
  vi.mocked(closeBrowserTab).mockResolvedValue(emptyState());
  vi.mocked(createBrowserTab).mockResolvedValue(emptyState());
  vi.mocked(browserBack).mockResolvedValue(stateWith([tab()]));
  vi.mocked(browserForward).mockResolvedValue(stateWith([tab()]));
  vi.mocked(browserReload).mockResolvedValue(stateWith([tab()]));
  vi.mocked(setupBrowser).mockResolvedValue({
    ...emptyState(),
    setup: {
      ...emptyState().setup,
      phase: 'checking',
    },
  });
});

describe('BrowserPanel', () => {
  it('opens an address and shows the page and its live view', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel session={makeSession()} />);

    const address = await screen.findByLabelText('Address');
    await user.type(address, 'http://localhost:3000/');
    await user.click(screen.getByRole('button', { name: 'Open' }));

    expect(openBrowserUrl).toHaveBeenCalledWith('s1', 'http://localhost:3000/');
    expect(await screen.findByRole('tab', { name: 'Demo' })).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Live view of Demo' }),
    ).toHaveAttribute(
      'src',
      '/api/sessions/s1/browser/screenshot?tab=s1-tab-1&v=1',
    );
  });

  it('shows a loading state until the page opens', async () => {
    const user = userEvent.setup();
    let finish: (state: BrowserState) => void = () => undefined;
    vi.mocked(openBrowserUrl).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    render(<BrowserPanel session={makeSession()} />);

    await user.type(await screen.findByLabelText('Address'), 'http://localhost:3000/');
    await user.click(screen.getByRole('button', { name: 'Open' }));

    expect(screen.getByRole('status')).toHaveTextContent('Loading page…');
    finish(stateWith([tab()]));
    expect(
      await screen.findByRole('img', { name: 'Live view of Demo' }),
    ).toBeInTheDocument();
  });

  it('goes back, forward and reloads the active tab', async () => {
    vi.mocked(getBrowserState).mockResolvedValue(
      stateWith([tab({ canGoBack: true, canGoForward: false })]),
    );
    const user = userEvent.setup();
    render(<BrowserPanel session={makeSession()} />);

    const back = await screen.findByRole('button', { name: 'Back' });
    const forward = screen.getByRole('button', { name: 'Forward' });
    const reload = screen.getByRole('button', { name: 'Reload' });
    expect(back).toBeEnabled();
    expect(forward).toBeDisabled();
    expect(reload).toBeEnabled();

    await user.click(back);
    expect(browserBack).toHaveBeenCalledWith('s1');

    await user.click(reload);
    expect(browserReload).toHaveBeenCalledWith('s1');
  });

  it('switches, adds and closes tabs', async () => {
    vi.mocked(getBrowserState).mockResolvedValue(
      stateWith([
        tab(),
        tab({
          id: 's1-tab-2',
          url: 'http://localhost:3000/other',
          title: 'Other',
        }),
      ]),
    );
    const user = userEvent.setup();
    render(<BrowserPanel session={makeSession()} />);

    await user.click(await screen.findByRole('tab', { name: 'Other' }));
    expect(activateBrowserTab).toHaveBeenCalledWith('s1', 's1-tab-2');

    await user.click(screen.getByRole('button', { name: 'Close tab Demo' }));
    expect(closeBrowserTab).toHaveBeenCalledWith('s1', 's1-tab-1');

    await user.click(screen.getByRole('button', { name: 'New tab' }));
    expect(createBrowserTab).toHaveBeenCalledWith('s1');
  });

  it('keeps each session on its own tab state', async () => {
    const other: Session = makeSession({ id: 's2', name: 'Other session' });
    vi.mocked(getBrowserState).mockImplementation(async (id) =>
      id === 's1'
        ? stateWith([tab({ id: 's1-tab-1', title: 'First page' })])
        : stateWith([
            tab({ id: 's2-tab-1', title: 'Second page', url: 'http://localhost:4000/' }),
          ]),
    );
    const { rerender } = render(<BrowserPanel session={makeSession()} />);
    expect(await screen.findByRole('tab', { name: 'First page' })).toBeInTheDocument();

    rerender(<BrowserPanel session={other} />);
    expect(await screen.findByRole('tab', { name: 'Second page' })).toBeInTheDocument();
    expect(screen.queryByRole('tab', { name: 'First page' })).not.toBeInTheDocument();
    expect(getBrowserState).toHaveBeenCalledWith('s2');
  });

  it('reports an error from the browser', async () => {
    vi.mocked(getBrowserState).mockResolvedValue({
      ...emptyState(),
      available: false,
      error: 'The browser is not available.',
    });
    render(<BrowserPanel session={makeSession()} />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The browser is not available.',
    );
    await waitFor(() => expect(getBrowserState).toHaveBeenCalledWith('s1'));
  });

  it('shows the browser download progress', async () => {
    vi.mocked(getBrowserState).mockResolvedValue({
      ...emptyState(),
      available: false,
      setup: {
        phase: 'downloading',
        downloaded: 50,
        total: 200,
        version: null,
        source: null,
        error: null,
      },
    });
    render(<BrowserPanel session={makeSession()} />);

    const status = await screen.findByRole('status');
    expect(status).toHaveTextContent('Downloading Chrome for Testing…');
    expect(status).toHaveTextContent('25% downloaded');
    expect(screen.getByRole('progressbar')).toHaveAttribute('value', '50');
    expect(screen.getByRole('button', { name: 'Open' })).toBeDisabled();
  });

  it('shows a failed browser download and retries it', async () => {
    const user = userEvent.setup();
    vi.mocked(getBrowserState).mockResolvedValue({
      ...emptyState(),
      available: false,
      setup: {
        phase: 'failed',
        downloaded: 0,
        total: null,
        version: null,
        source: null,
        error: 'The browser download failed with status 500.',
      },
    });
    render(<BrowserPanel session={makeSession()} />);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('The browser could not be installed.');
    expect(alert).toHaveTextContent(
      'The browser download failed with status 500.',
    );

    await user.click(screen.getByRole('button', { name: 'Retry' }));
    expect(setupBrowser).toHaveBeenCalledWith('s1');
    expect(
      await screen.findByText('Checking for an installed browser…'),
    ).toBeInTheDocument();
  });
});
