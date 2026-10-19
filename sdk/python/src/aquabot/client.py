"""Low-level client of the Aquamarine Bot API (docs/BOTS.md).

>>> api = Client("https://relay.example", token)
>>> api.me()
{'username': 'weatherbot', 'display_name': 'Weather'}
>>> for u in api.updates(after=0, timeout=25): ...
>>> api.send("alice", "hi")
"""
from __future__ import annotations

import os
import time
from dataclasses import dataclass, field
from typing import List, Optional

from ._http import HTTPError, request

#: The relay refuses longer texts (413).
MAX_TEXT = 4096


class BotError(Exception):
    """Base class of the errors below."""


class Unauthorized(BotError):
    """401: the token is wrong or was revoked. Stop the bot."""


class Forbidden(BotError):
    """403: the user hasn't written to the bot yet, or sent /stop."""


class RateLimited(BotError):
    """429 after all retries."""


@dataclass
class Update:
    """One incoming message."""

    id: int
    sender: str
    text: str
    ts: int = 0
    raw: dict = field(default_factory=dict, repr=False)

    @property
    def is_command(self) -> bool:
        return self.text.startswith("/")

    @property
    def command(self) -> str:
        """"/ask@bot hello" → "ask" ("" when not a command)."""
        if not self.is_command:
            return ""
        return self.text[1:].split(maxsplit=1)[0].split("@", 1)[0].lower() if len(self.text) > 1 else ""

    @property
    def args(self) -> str:
        """Text after the command ("/ask hello there" → "hello there")."""
        parts = self.text.split(maxsplit=1)
        return parts[1].strip() if self.is_command and len(parts) > 1 else ("" if self.is_command else self.text)


def split_text(text: str, limit: int = MAX_TEXT) -> List[str]:
    """Cut a long text into pieces ≤ limit, preferring paragraph, line and word breaks."""
    text = text or ""
    out: List[str] = []
    while len(text) > limit:
        cut = -1
        for sep in ("\n\n", "\n", ". ", " "):
            cut = text.rfind(sep, 0, limit)
            if cut > limit // 2:
                cut += len(sep)
                break
        if cut <= 0:
            cut = limit
        out.append(text[:cut].rstrip())
        text = text[cut:].lstrip()
    if text:
        out.append(text)
    return out or [""]


class Client:
    """Thin, retrying wrapper over the HTTP API.

    ``server`` defaults to ``$AQUA_SERVER``, ``token`` to ``$AQUA_BOT_TOKEN``.
    """

    def __init__(self, server: Optional[str] = None, token: Optional[str] = None, *, retries: int = 4):
        self.server = (server or os.environ.get("AQUA_SERVER") or "http://127.0.0.1:7777").rstrip("/")
        self.token = token or os.environ.get("AQUA_BOT_TOKEN") or ""
        if not self.token:
            raise Unauthorized("no bot token: pass token=… or set AQUA_BOT_TOKEN")
        self.retries = retries

    # -- plumbing --------------------------------------------------------------
    def _call(self, method: str, path: str, body=None, timeout: float = 40):
        url = self.server + path
        delay = 1.1
        for attempt in range(self.retries + 1):
            try:
                return request(method, url, headers={"Authorization": "Bot " + self.token}, body=body, timeout=timeout)
            except HTTPError as e:
                if e.status == 401:
                    raise Unauthorized(e.body or "token rejected") from None
                if e.status == 403:
                    raise Forbidden(e.body or "not allowed") from None
                if e.status == 429 and attempt < self.retries:
                    time.sleep(delay)
                    delay = min(delay * 1.6, 8)
                    continue
                if e.status == 429:
                    raise RateLimited(e.body) from None
                if e.status >= 500 and attempt < self.retries:
                    time.sleep(delay)
                    continue
                raise
        raise RateLimited("gave up")

    # -- API -------------------------------------------------------------------
    def me(self) -> dict:
        """``{"username", "display_name"}`` — also checks the token."""
        return self._call("GET", "/v1/bot/me")

    def updates(self, after: int = 0, timeout: int = 25) -> List[Update]:
        """Long-poll for new messages. ``after`` acknowledges everything before it."""
        timeout = max(0, min(int(timeout), 30))
        rows = self._call("GET", f"/v1/bot/updates?after={int(after)}&timeout={timeout}", timeout=timeout + 15) or []
        return [Update(id=int(r.get("id", 0)), sender=str(r.get("from", "")), text=str(r.get("text", "")),
                       ts=int(r.get("ts", 0) or 0), raw=r) for r in rows]

    def send(self, to: str, text: str) -> int:
        """Send ``text`` (split into several messages if longer than 4096). Returns how many were sent."""
        parts = [p for p in split_text(text) if p.strip()]
        for i, part in enumerate(parts):
            self._call("POST", "/v1/bot/send", body={"to": to, "text": part})
            if i + 1 < len(parts):
                time.sleep(1.05)   # 1 message/s per recipient (burst 5)
        return len(parts)
