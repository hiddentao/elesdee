import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resizeSession, sendInput } from '../api/client';
import { FakeEventSource } from '../test/fakeEventSource';
import { makeSession } from '../test/fixtures';
import { TerminalPane } from './TerminalPane';

const mocks = vi.hoisted(() => {
  const write = vi.fn();
  const writeln = vi.fn();
  const open = vi.fn();
  const loadAddon = vi.fn();
  const dispose = vi.fn();
  const onData = vi.fn();
  const fit = vi.fn();
  const terminals: unknown[] = [];
  const dataHandler = { current: null as ((data: string) => void) | null };

  return {
    write,
    writeln,
    open,
    loadAddon,
    dispose,
    onData,
    fit,
    terminals,
    dataHandler,
  };
});

vi.mock('@xterm/xterm', () => {
  class FakeTerminal {
    cols = 80;
    rows = 24;
    options: Record<string, unknown> = {};
    write = mocks.write;
    writeln = mocks.writeln;
    open = mocks.open;
    loadAddon = mocks.loadAddon;
    onData = mocks.onData;
    dispose = mocks.dispose;

    constructor(options: unknown) {
      Object.assign(this.options, options as Record<string, unknown>);
      mocks.terminals.push(this);
    }
  }

  return { Terminal: FakeTerminal };
});

vi.mock('@xterm/addon-fit', () => {
  class FakeFitAddon {
    fit = mocks.fit;
  }

  return { FitAddon: FakeFitAddon };
});

vi.mock('../api/client', () => ({
  sendInput: vi.fn(),
  resizeSession: vi.fn(),
}));

const observers: (() => void)[] = [];

class FakeResizeObserver {
  constructor(callback: () => void) {
    observers.push(callback);
  }

  observe() {}

  disconnect() {}
}

interface FakeTerm {
  cols: number;
  rows: number;
  options: Record<string, unknown>;
  write: ReturnType<typeof vi.fn>;
  writeln: ReturnType<typeof vi.fn>;
}

const activeSession = makeSession({
  name: 'Terminal check',
  short: 'terminal-check',
  kind: 'terminal',
  client: null,
  accountId: null,
});

function term(): FakeTerm {
  return mocks.terminals[0] as FakeTerm;
}

beforeEach(() => {
  FakeEventSource.reset();
  observers.length = 0;
  mocks.terminals.length = 0;
  mocks.dataHandler.current = null;
  mocks.onData.mockImplementation((handler: (data: string) => void) => {
    mocks.dataHandler.current = handler;
    return { dispose: vi.fn() };
  });
  vi.mocked(sendInput).mockResolvedValue(undefined);
  vi.mocked(resizeSession).mockResolvedValue(undefined);
  vi.stubGlobal('EventSource', FakeEventSource);
  vi.stubGlobal('ResizeObserver', FakeResizeObserver);
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('TerminalPane', () => {
  it('creates a fitted terminal with the expected options', () => {
    render(<TerminalPane session={activeSession} />);

    expect(term().options).toMatchObject({
      scrollback: 5000,
      cursorBlink: true,
      fontSize: 13,
      lineHeight: 1.55,
      screenReaderMode: true,
    });
    expect(mocks.fit).toHaveBeenCalled();
    expect(resizeSession).toHaveBeenCalledWith('s1', 80, 24);
  });

  it('sends raw terminal input', () => {
    render(<TerminalPane session={activeSession} />);

    act(() => {
      mocks.dataHandler.current?.('ls\r');
    });

    expect(sendInput).toHaveBeenCalledWith('s1', 'ls\r');
  });

  it('sends a new size after the container resizes', () => {
    render(<TerminalPane session={activeSession} />);

    term().cols = 100;
    term().rows = 30;
    act(() => {
      observers[0]();
    });

    expect(resizeSession).toHaveBeenLastCalledWith('s1', 100, 30);
  });

  it('writes raw output chunks to the terminal', () => {
    render(<TerminalPane session={activeSession} />);
    const source = FakeEventSource.instances[0];

    act(() => {
      source.open();
      source.message({ seq: 1, data: '\x1b[32mhello\x1b[0m' });
    });

    expect(mocks.write).toHaveBeenCalledWith('\x1b[32mhello\x1b[0m');
  });

  it('writes a line when the server reports a gap', () => {
    render(<TerminalPane session={activeSession} />);
    const source = FakeEventSource.instances[0];

    act(() => {
      source.message({
        seq: 1,
        data: 'tail',
        gap: { after: 0, oldestSeq: 10, newestSeq: 20 },
      });
    });

    expect(mocks.writeln).toHaveBeenCalledWith(
      expect.stringContaining('Earlier output'),
    );
    expect(mocks.write).toHaveBeenCalledWith('tail');
  });

  it('loads a closed log once and shows no disconnect banner', () => {
    render(<TerminalPane session={{ ...activeSession, status: 'closed' }} />);
    const source = FakeEventSource.instances[0];

    act(() => {
      source.open();
      source.message({ seq: 1, data: 'old log' });
      source.fail();
    });

    expect(mocks.write).toHaveBeenCalledWith('old log');
    expect(
      screen.queryByText('Output is disconnected.'),
    ).not.toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(FakeEventSource.instances).toHaveLength(1);
  });

  it('reconnects from the last sequence when Retry is clicked', () => {
    render(<TerminalPane session={activeSession} />);
    const source = FakeEventSource.instances[0];

    act(() => {
      source.open();
      source.message({ seq: 1, data: 'one' });
      source.message({ seq: 2, data: 'two' });
      source.fail();
    });
    expect(screen.getByText('Output is disconnected.')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(
      screen.queryByText('Output is disconnected.'),
    ).not.toBeInTheDocument();
    expect(FakeEventSource.instances).toHaveLength(2);
    expect(FakeEventSource.instances[1].url).toContain('after=2');

    act(() => {
      FakeEventSource.instances[1].open();
    });
    expect(screen.getByText('Output reconnected.')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(FakeEventSource.instances).toHaveLength(2);
  });
});
