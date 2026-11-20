import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeEventSource } from '../test/fakeEventSource';
import { subscribeHistory, subscribeOutput } from './events';

beforeEach(() => {
  FakeEventSource.reset();
  vi.stubGlobal('EventSource', FakeEventSource);
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('output stream', () => {
  it('reconnects from the last sequence for an active session', () => {
    const onChunk = vi.fn();
    const onError = vi.fn();
    const stop = subscribeOutput('s1', 0, onChunk, onError);

    const first = FakeEventSource.instances[0];
    expect(first.url).toContain('/api/sessions/s1/output?after=0');

    first.open();
    first.message({ seq: 1, data: 'hello' });
    expect(onChunk).toHaveBeenCalledWith(1, 'hello');

    first.fail();
    expect(onError).toHaveBeenCalledTimes(1);
    expect(first.closed).toBe(true);

    vi.advanceTimersByTime(500);
    expect(FakeEventSource.instances).toHaveLength(2);
    expect(FakeEventSource.instances[1].url).toContain('after=1');

    stop();
  });

  it('loads a closed log once without an error or reconnect', () => {
    const onChunk = vi.fn();
    const onError = vi.fn();
    const stop = subscribeOutput('s2', 0, onChunk, onError, undefined, () => true);

    const source = FakeEventSource.instances[0];
    source.open();
    source.message({ seq: 1, data: 'old log' });
    source.fail();

    expect(onChunk).toHaveBeenCalledWith(1, 'old log');
    expect(onError).not.toHaveBeenCalled();
    expect(source.closed).toBe(true);

    vi.advanceTimersByTime(10000);
    expect(FakeEventSource.instances).toHaveLength(1);

    stop();
  });

  it('passes a gap marker to the chunk handler', () => {
    const onChunk = vi.fn();
    const stop = subscribeOutput('s4', 0, onChunk);

    const source = FakeEventSource.instances[0];
    source.message({
      seq: 1,
      data: 'tail',
      gap: { after: 0, oldestSeq: 10, newestSeq: 20 },
    });

    expect(onChunk).toHaveBeenCalledWith(1, 'tail', {
      after: 0,
      oldestSeq: 10,
      newestSeq: 20,
    });

    stop();
  });

  it('stops when the session closes during the stream', () => {
    let closed = false;
    const onError = vi.fn();
    const stop = subscribeOutput('s3', 0, vi.fn(), onError, undefined, () => closed);

    const source = FakeEventSource.instances[0];
    source.open();
    closed = true;
    source.fail();

    expect(onError).not.toHaveBeenCalled();

    vi.advanceTimersByTime(10000);
    expect(FakeEventSource.instances).toHaveLength(1);

    stop();
  });
});

describe('history stream', () => {
  it('reads the status notice, replays items and reconnects after the last one', () => {
    const onStatus = vi.fn();
    const onItem = vi.fn();
    const onError = vi.fn();
    const stop = subscribeHistory('s1', 0, onStatus, onItem, onError);

    const first = FakeEventSource.instances[0];
    expect(first.url).toContain('/api/sessions/s1/history?after=0');

    first.open();
    first.message({
      type: 'history.state',
      structured: true,
      client: 'Claude Code',
    });
    expect(onStatus).toHaveBeenCalledWith({
      type: 'history.state',
      structured: true,
      client: 'Claude Code',
    });

    first.message({
      seq: 1,
      id: 'h1',
      kind: 'prompt',
      text: 'Fix it',
      state: 'done',
    });
    expect(onItem).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'h1', text: 'Fix it' }),
    );

    first.message({
      seq: 2,
      id: 'h2',
      kind: 'reply',
      text: 'Working',
      state: 'streaming',
    });
    expect(onItem).toHaveBeenCalledTimes(2);

    first.fail();
    expect(onError).toHaveBeenCalledTimes(1);

    vi.advanceTimersByTime(500);
    expect(FakeEventSource.instances).toHaveLength(2);
    expect(FakeEventSource.instances[1].url).toContain('after=2');

    stop();
  });

  it('ignores malformed items and keeps the stream open', () => {
    const onItem = vi.fn();
    const stop = subscribeHistory('s2', 0, vi.fn(), onItem);

    const source = FakeEventSource.instances[0];
    source.message({ seq: 1, id: 'h1', kind: 'tool' });
    source.message({ seq: 2, kind: 'prompt', text: 'no id' });

    expect(onItem).not.toHaveBeenCalled();

    stop();
  });
});
