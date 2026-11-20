import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  activateBrowserTab,
  browserBack,
  browserForward,
  browserReload,
  browserScreenshotUrl,
  clickBrowserAt,
  closeBrowserTab,
  createBrowserTab,
  getBrowserState,
  openBrowserUrl,
  pressBrowserKey,
  setupBrowser,
  typeBrowserText,
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
  clickBrowserAt: vi.fn(),
  closeBrowserTab: vi.fn(),
  createBrowserTab: vi.fn(),
  getBrowserState: vi.fn(),
  openBrowserUrl: vi.fn(),
  pressBrowserKey: vi.fn(),
  setupBrowser: vi.fn(),
  typeBrowserText: vi.fn(),
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

afterEach(() => {
  vi.restoreAllMocks();
});

beforeEach(() => {
  vi.clearAllMocks();
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
  vi.mocked(clickBrowserAt).mockResolvedValue(stateWith([tab()]));
  vi.mocked(typeBrowserText).mockResolvedValue(stateWith([tab()]));
  vi.mocked(pressBrowserKey).mockResolvedValue(stateWith([tab()]));
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

  it('moves browser tabs with arrow keys and one tab stop', async () => {
    const tabs = [
      tab(),
      tab({ id: 's1-tab-2', title: 'Other', url: 'http://localhost:3000/other' }),
      tab({ id: 's1-tab-3', title: 'Third', url: 'http://localhost:3000/third' }),
    ];
    vi.mocked(getBrowserState).mockResolvedValue(stateWith(tabs));
    vi.mocked(activateBrowserTab).mockImplementation(async (_id, tabId) =>
      stateWith(tabs, { activeTabId: tabId }),
    );
    const user = userEvent.setup();
    render(<BrowserPanel session={makeSession()} />);

    const first = await screen.findByRole('tab', { name: 'Demo' });
    const second = screen.getByRole('tab', { name: 'Other' });
    const third = screen.getByRole('tab', { name: 'Third' });
    expect(first).toHaveAttribute('tabindex', '0');
    expect(second).toHaveAttribute('tabindex', '-1');
    expect(third).toHaveAttribute('tabindex', '-1');

    first.focus();
    await user.keyboard('{ArrowRight}');
    expect(activateBrowserTab).toHaveBeenCalledWith('s1', 's1-tab-2');
    await waitFor(() => expect(second).toHaveFocus());

    await user.keyboard('{End}');
    expect(activateBrowserTab).toHaveBeenLastCalledWith('s1', 's1-tab-3');
    await waitFor(() => expect(third).toHaveFocus());

    await user.keyboard('{Home}');
    expect(activateBrowserTab).toHaveBeenLastCalledWith('s1', 's1-tab-1');
    await waitFor(() => expect(first).toHaveFocus());
  });

  it('leaves arrow keys alone on the close and new-tab buttons', async () => {
    vi.mocked(getBrowserState).mockResolvedValue(
      stateWith([
        tab(),
        tab({
          id: 's1-tab-2',
          title: 'Other',
          url: 'http://localhost:3000/other',
        }),
      ]),
    );
    const user = userEvent.setup();
    render(<BrowserPanel session={makeSession()} />);

    const close = await screen.findByRole('button', {
      name: 'Close tab Demo',
    });
    close.focus();
    await user.keyboard('{ArrowRight}');
    expect(activateBrowserTab).not.toHaveBeenCalled();
    expect(close).toHaveFocus();

    const newTab = screen.getByRole('button', { name: 'New tab' });
    newTab.focus();
    await user.keyboard('{ArrowLeft}');
    expect(activateBrowserTab).not.toHaveBeenCalled();
    expect(newTab).toHaveFocus();
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

  it('clicks the live view at the matching point of the page', async () => {
    vi.mocked(getBrowserState).mockResolvedValue(stateWith([tab()]));
    render(<BrowserPanel session={makeSession()} />);

    const live = await screen.findByRole('application', {
      name: 'Live browser page',
    });
    live.getBoundingClientRect = () => ({
      left: 100,
      top: 50,
      right: 500,
      bottom: 350,
      width: 400,
      height: 300,
      x: 100,
      y: 50,
      toJSON: () => ({}),
    });

    fireEvent.click(live, { clientX: 300, clientY: 125 });
    await waitFor(() =>
      expect(clickBrowserAt).toHaveBeenCalledWith('s1', 0.5, 0.25),
    );
  });

  it('types text and presses named keys in the live view', async () => {
    vi.mocked(getBrowserState).mockResolvedValue(stateWith([tab()]));
    render(<BrowserPanel session={makeSession()} />);

    const live = await screen.findByRole('application', {
      name: 'Live browser page',
    });

    fireEvent.keyDown(live, { key: 'a' });
    fireEvent.keyDown(live, { key: 'b' });
    await waitFor(() =>
      expect(typeBrowserText).toHaveBeenCalledWith('s1', 'ab'),
    );

    fireEvent.keyDown(live, { key: 'Enter' });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'Enter', 0),
    );

    fireEvent.keyDown(live, { key: 'Tab', shiftKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'Tab', 8),
    );

    fireEvent.keyDown(live, { key: 'Escape' });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'Escape', 0),
    );

    expect(typeBrowserText).toHaveBeenCalledTimes(1);
  });

  it('sends modified letter and digit chords to the page', async () => {
    vi.mocked(getBrowserState).mockResolvedValue(stateWith([tab()]));
    render(<BrowserPanel session={makeSession()} />);

    const live = await screen.findByRole('application', {
      name: 'Live browser page',
    });

    expect(fireEvent.keyDown(live, { key: 'a', ctrlKey: true })).toBe(false);
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'a', 2),
    );

    fireEvent.keyDown(live, { key: 'c', metaKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'c', 4),
    );

    fireEvent.keyDown(live, { key: '1', altKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', '1', 1),
    );

    fireEvent.keyDown(live, { key: 'a', ctrlKey: true, shiftKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'A', 10),
    );

    fireEvent.keyDown(live, { key: 'ArrowLeft', altKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'ArrowLeft', 1),
    );

    fireEvent.keyDown(live, { key: 'Home', ctrlKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'Home', 2),
    );

    expect(typeBrowserText).not.toHaveBeenCalled();
  });

  it('leaves the app window shortcuts to the app on a Mac', async () => {
    const userAgent = vi
      .spyOn(navigator, 'userAgent', 'get')
      .mockReturnValue(
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/537.36',
      );
    vi.mocked(getBrowserState).mockResolvedValue(stateWith([tab()]));
    render(<BrowserPanel session={makeSession()} />);

    const live = await screen.findByRole('application', {
      name: 'Live browser page',
    });

    for (const key of ['q', 'w', 'm', 'h']) {
      expect(fireEvent.keyDown(live, { key, metaKey: true })).toBe(true);
    }
    await new Promise((resolve) => window.setTimeout(resolve, 60));
    expect(pressBrowserKey).not.toHaveBeenCalled();

    fireEvent.keyDown(live, { key: 'a', metaKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'a', 4),
    );
    expect(pressBrowserKey).toHaveBeenCalledTimes(1);

    userAgent.mockRestore();
  });

  it('leaves the window close shortcut to the app on other platforms', async () => {
    const userAgent = vi
      .spyOn(navigator, 'userAgent', 'get')
      .mockReturnValue('Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36');
    vi.mocked(getBrowserState).mockResolvedValue(stateWith([tab()]));
    render(<BrowserPanel session={makeSession()} />);

    const live = await screen.findByRole('application', {
      name: 'Live browser page',
    });

    expect(fireEvent.keyDown(live, { key: 'w', ctrlKey: true })).toBe(true);
    await new Promise((resolve) => window.setTimeout(resolve, 60));
    expect(pressBrowserKey).not.toHaveBeenCalled();

    fireEvent.keyDown(live, { key: 'a', ctrlKey: true });
    await waitFor(() =>
      expect(pressBrowserKey).toHaveBeenCalledWith('s1', 'a', 2),
    );
    expect(pressBrowserKey).toHaveBeenCalledTimes(1);

    userAgent.mockRestore();
  });
});
