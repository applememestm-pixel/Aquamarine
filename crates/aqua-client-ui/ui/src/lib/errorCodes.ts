/**
 * Error codes — the same list as docs/errors.md ("what to do", for people
 * and for whoever runs the server). A failure is matched by its text;
 * reports carry the code, and `errorHelp` gives the page to open.
 */
export type ErrorCode = { code: string; re: RegExp };

export const ERROR_CODES: ErrorCode[] = [
  { code: "NET-REFUSED", re: /ECONNREFUSED|10061|Connection refused|actively refused|отверг/i },
  { code: "NET-TIMEOUT", re: /timed out|timeout|ETIMEDOUT|10060/i },
  { code: "NET-DNS", re: /dns|resolve|ENOTFOUND|No such host|getaddrinfo/i },
  { code: "NET-TLS", re: /tls|ssl|certificate|InvalidContentType|wrong version number/i },
  { code: "NET-OFFLINE", re: /network is unreachable|ENETUNREACH|no route to host|offline/i },
  { code: "SRV-OLD", re: /no longer supported|426|update the app/i },
  { code: "SRV-5XX", re: /\b50[0-4]\b|Internal Server Error|Bad Gateway|Service Unavailable/i },
  { code: "AUTH-2FA", re: /two-step|2fa|totp|need_2fa/i },
  { code: "AUTH-DEVICE", re: /DeviceBound|bound to (the|its) device/i },
  { code: "AUTH-RATE", re: /\b429\b|too many/i },
  { code: "TG-WEBHOOK", re: /webhook/i },
  { code: "TG-CONFLICT", re: /another program reads this Telegram bot|terminated by other getUpdates/i },
  { code: "TG-TOKEN", re: /bot token is invalid|Unauthorized/i },
  { code: "FILE-BIG", re: /too big|too large|413/i },
  { code: "MEDIA", re: /MEDIA_ERR|decode|NotSupportedError|no supported source/i },
];

export function codeOf(msg: string): string {
  return ERROR_CODES.find((c) => c.re.test(msg))?.code ?? "";
}

export const GUIDE = "https://applememestm-pixel.github.io/Aquamarine/docs.html?d=errors";
export function errorHelp(msg: string): { code: string; url: string } | null {
  const code = codeOf(msg);
  return code ? { code, url: `${GUIDE}#${code.toLowerCase()}` } : null;
}
