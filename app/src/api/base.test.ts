import { describe, expect, it } from 'vitest';
import { apiBase } from './base';

describe('apiBase', () => {
  it('uses the dev proxy from a browser origin', () => {
    expect(apiBase('http:', '127.0.0.1')).toBe('');
    expect(apiBase('http:', 'localhost')).toBe('');
  });

  it('uses the loopback core from the Tauri asset origin', () => {
    expect(apiBase('tauri:', 'localhost')).toBe('http://127.0.0.1:4319');
    expect(apiBase('https:', 'tauri.localhost')).toBe('http://127.0.0.1:4319');
  });
});
