import { describe, expect, it } from 'vitest';
import { isOpenableDemoAddress } from './demoAddress';

describe('isOpenableDemoAddress', () => {
  it('accepts the local loopback addresses', () => {
    expect(isOpenableDemoAddress('http://localhost:3000')).toBe(true);
    expect(isOpenableDemoAddress('http://127.0.0.1:8080')).toBe(true);
    expect(isOpenableDemoAddress('http://[::1]:8080/')).toBe(true);
  });

  it('accepts MagicDNS and tailnet addresses', () => {
    expect(
      isOpenableDemoAddress('https://studio.tailnet.ts.net:3000'),
    ).toBe(true);
    expect(isOpenableDemoAddress('http://100.64.0.1:3000')).toBe(true);
    expect(isOpenableDemoAddress('http://100.127.255.255:3000')).toBe(true);
  });

  it('refuses another host or scheme', () => {
    expect(isOpenableDemoAddress('https://example.com:3000')).toBe(false);
    expect(isOpenableDemoAddress('https://notts.net:3000')).toBe(false);
    expect(isOpenableDemoAddress('http://100.63.255.255:3000')).toBe(false);
    expect(isOpenableDemoAddress('http://100.128.0.0:3000')).toBe(false);
    expect(isOpenableDemoAddress('javascript:alert(1)')).toBe(false);
    expect(isOpenableDemoAddress('file:///etc/passwd')).toBe(false);
    expect(isOpenableDemoAddress('http://user@localhost:3000')).toBe(false);
    expect(isOpenableDemoAddress('not a url')).toBe(false);
  });
});
