"""Tiny JSON-over-HTTP helper on the standard library (no dependencies)."""
from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any, Dict, Iterator, Optional

USER_AGENT = "aquamarine-bot/0.1 (+https://github.com/applememestm-pixel/aquamarine)"


class HTTPError(Exception):
    """A non-2xx answer. ``status`` is the HTTP code, ``body`` the response text."""

    def __init__(self, status: int, body: str, url: str = ""):
        super().__init__(f"HTTP {status}: {body[:300]}")
        self.status = status
        self.body = body
        self.url = url


def request(
    method: str,
    url: str,
    *,
    headers: Optional[Dict[str, str]] = None,
    body: Any = None,
    timeout: float = 60,
) -> Any:
    """Send a request; return the decoded JSON (or None for an empty body)."""
    data = None
    h = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        h["Content-Type"] = "application/json"
    h.update(headers or {})
    req = urllib.request.Request(url, data=data, method=method, headers=h)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
    except urllib.error.HTTPError as e:
        raise HTTPError(e.code, e.read().decode("utf-8", "replace"), url) from None
    if not raw:
        return None
    try:
        return json.loads(raw)
    except ValueError:
        return raw.decode("utf-8", "replace")


def stream_lines(
    method: str,
    url: str,
    *,
    headers: Optional[Dict[str, str]] = None,
    body: Any = None,
    timeout: float = 300,
) -> Iterator[str]:
    """Yield the response line by line (SSE ``data:`` lines, NDJSON…)."""
    h = {"User-Agent": USER_AGENT, "Accept": "text/event-stream, application/x-ndjson, application/json"}
    data = None
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        h["Content-Type"] = "application/json"
    h.update(headers or {})
    req = urllib.request.Request(url, data=data, method=method, headers=h)
    try:
        r = urllib.request.urlopen(req, timeout=timeout)
    except urllib.error.HTTPError as e:
        raise HTTPError(e.code, e.read().decode("utf-8", "replace"), url) from None
    with r:
        for raw in r:
            line = raw.decode("utf-8", "replace").rstrip("\r\n")
            if line:
                yield line
