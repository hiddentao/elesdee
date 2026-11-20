/** The hosts whose demo-server address the panel may open. */
export function isOpenableDemoAddress(value: string): boolean {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return false;
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    return false;
  }
  if (url.username || url.password) {
    return false;
  }
  const host = url.hostname.toLowerCase();
  if (host === 'localhost' || host === '127.0.0.1' || host === '[::1]') {
    return true;
  }
  if (host.endsWith('.ts.net') && host.length > '.ts.net'.length) {
    return true;
  }
  return isTailnetIpv4(host);
}

/**
 * Checks the Tailscale address range 100.64.0.0/10.
 */
function isTailnetIpv4(host: string): boolean {
  const parts = host.split('.');
  if (parts.length !== 4) {
    return false;
  }
  const octets = parts.map((part) => (/^\d{1,3}$/.test(part) ? Number(part) : NaN));
  if (octets.some((octet) => !Number.isInteger(octet) || octet > 255)) {
    return false;
  }
  return octets[0] === 100 && (octets[1] & 0xc0) === 64;
}
