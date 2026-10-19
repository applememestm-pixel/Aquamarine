"""Language-model connectors with one interface.

Every connector has:

* ``chat(messages) -> str`` — one answer;
* ``stream(messages) -> Iterator[str]`` — the answer piece by piece;
* ``ask(prompt, system=None) -> str`` — shortcut for a single question.

``messages`` is a list of ``{"role": "system" | "user" | "assistant", "content": str}``.

Connectors:

* :class:`LlamaCpp` — a local ``llama-server`` (llama.cpp), default ``http://127.0.0.1:8080``;
* :class:`OpenAICompatible` — anything speaking ``/v1/chat/completions``: OpenAI,
  OpenRouter, Groq, Together, LM Studio, vLLM, LocalAI, llama.cpp…;
* :class:`Ollama` — a local Ollama, default ``http://127.0.0.1:11434``;
* :class:`Anthropic` — the Anthropic Messages API.

:func:`connect` builds one from a short spec or the environment:

    connect("llamacpp")                         # http://127.0.0.1:8080
    connect("llamacpp:http://gpu-box:8080")
    connect("ollama:llama3.1")
    connect("openai:gpt-4o-mini")               # key from $OPENAI_API_KEY
    connect("openai-compatible:http://127.0.0.1:1234/v1:qwen2.5-7b")
    connect("anthropic:<model>")                # key from $ANTHROPIC_API_KEY
    connect()                                   # from $AQUA_LLM (+ $AQUA_LLM_URL / _MODEL / _KEY)
"""
from __future__ import annotations

import json
import os
import time
from typing import Any, Dict, Iterator, List, Optional

from ._http import HTTPError, request, stream_lines

Message = Dict[str, str]


class LLMError(Exception):
    """The model server refused or failed."""


def _retry(fn, attempts: int = 3):
    delay = 1.0
    for i in range(attempts):
        try:
            return fn()
        except HTTPError as e:
            if e.status in (429, 500, 502, 503, 504) and i + 1 < attempts:
                time.sleep(delay)
                delay *= 2
                continue
            raise LLMError(f"{e.status}: {e.body[:500]}") from None
        except OSError as e:
            if i + 1 < attempts:
                time.sleep(delay)
                delay *= 2
                continue
            raise LLMError(f"can't reach the model server: {e}") from None


class LLM:
    """Base class: implement :meth:`chat` and :meth:`stream`."""

    temperature: Optional[float] = None
    max_tokens: Optional[int] = None

    def chat(self, messages: List[Message], **opts: Any) -> str:  # pragma: no cover - abstract
        raise NotImplementedError

    def stream(self, messages: List[Message], **opts: Any) -> Iterator[str]:
        yield self.chat(messages, **opts)

    def ask(self, prompt: str, system: Optional[str] = None, **opts: Any) -> str:
        msgs: List[Message] = [{"role": "system", "content": system}] if system else []
        msgs.append({"role": "user", "content": prompt})
        return self.chat(msgs, **opts)


# --------------------------------------------------------------------- OpenAI-style
class OpenAICompatible(LLM):
    """``POST {base_url}/chat/completions`` (``base_url`` usually ends with ``/v1``)."""

    def __init__(self, base_url: str, model: str = "", api_key: Optional[str] = None, *,
                 temperature: Optional[float] = 0.7, max_tokens: Optional[int] = None,
                 timeout: float = 300, extra: Optional[Dict[str, Any]] = None):
        base = base_url.rstrip("/")
        if not base.endswith("/v1") and "/v1/" not in base + "/":
            base += "/v1"
        self.base = base
        self.model = model
        self.api_key = api_key
        self.temperature = temperature
        self.max_tokens = max_tokens
        self.timeout = timeout
        self.extra = extra or {}

    def _headers(self) -> Dict[str, str]:
        return {"Authorization": f"Bearer {self.api_key}"} if self.api_key else {}

    def _body(self, messages: List[Message], stream: bool, opts: Dict[str, Any]) -> Dict[str, Any]:
        body: Dict[str, Any] = {"messages": messages, "stream": stream}
        if self.model:
            body["model"] = self.model
        t = opts.pop("temperature", self.temperature)
        if t is not None:
            body["temperature"] = t
        m = opts.pop("max_tokens", self.max_tokens)
        if m is not None:
            body["max_tokens"] = m
        body.update(self.extra)
        body.update(opts)
        return body

    def chat(self, messages: List[Message], **opts: Any) -> str:
        body = self._body(messages, False, dict(opts))
        r = _retry(lambda: request("POST", self.base + "/chat/completions", headers=self._headers(), body=body, timeout=self.timeout))
        try:
            return r["choices"][0]["message"]["content"] or ""
        except (KeyError, IndexError, TypeError):
            raise LLMError(f"unexpected answer: {str(r)[:300]}") from None

    def stream(self, messages: List[Message], **opts: Any) -> Iterator[str]:
        body = self._body(messages, True, dict(opts))
        try:
            for line in stream_lines("POST", self.base + "/chat/completions", headers=self._headers(), body=body, timeout=self.timeout):
                if not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                if data == "[DONE]":
                    break
                try:
                    piece = json.loads(data)["choices"][0].get("delta", {}).get("content")
                except (ValueError, KeyError, IndexError):
                    continue
                if piece:
                    yield piece
        except HTTPError as e:
            raise LLMError(f"{e.status}: {e.body[:500]}") from None

    def models(self) -> List[str]:
        r = _retry(lambda: request("GET", self.base + "/models", headers=self._headers(), timeout=30))
        return [m.get("id", "") for m in (r or {}).get("data", [])]

    def embed(self, text: str) -> List[float]:
        body: Dict[str, Any] = {"input": text}
        if self.model:
            body["model"] = self.model
        r = _retry(lambda: request("POST", self.base + "/embeddings", headers=self._headers(), body=body, timeout=self.timeout))
        return list(r["data"][0]["embedding"])


class LlamaCpp(OpenAICompatible):
    """A llama.cpp ``llama-server`` (``llama-server -m model.gguf --port 8080``).

    Uses its OpenAI-compatible chat endpoint (the server applies the model's
    chat template) plus the native ``/health``, ``/tokenize`` and ``/completion``.
    """

    def __init__(self, url: str = "http://127.0.0.1:8080", model: str = "", api_key: Optional[str] = None, **kw: Any):
        self.root = url.rstrip("/").removesuffix("/v1")
        super().__init__(self.root + "/v1", model=model, api_key=api_key, **kw)

    def health(self) -> bool:
        try:
            r = request("GET", self.root + "/health", headers=self._headers(), timeout=10)
            return (r or {}).get("status") in ("ok", None) if isinstance(r, dict) else True
        except (HTTPError, OSError):
            return False

    def tokenize(self, text: str) -> List[int]:
        r = _retry(lambda: request("POST", self.root + "/tokenize", headers=self._headers(), body={"content": text}, timeout=60))
        return list((r or {}).get("tokens", []))

    def complete(self, prompt: str, n_predict: int = 256, **opts: Any) -> str:
        """Raw text completion (no chat template)."""
        body = {"prompt": prompt, "n_predict": n_predict, **opts}
        if self.temperature is not None:
            body.setdefault("temperature", self.temperature)
        r = _retry(lambda: request("POST", self.root + "/completion", headers=self._headers(), body=body, timeout=self.timeout))
        return (r or {}).get("content", "")


# --------------------------------------------------------------------------- Ollama
class Ollama(LLM):
    """A local Ollama (``ollama serve``; ``ollama pull llama3.1``)."""

    def __init__(self, model: str, url: str = "http://127.0.0.1:11434", *, temperature: Optional[float] = 0.7,
                 max_tokens: Optional[int] = None, timeout: float = 300, options: Optional[Dict[str, Any]] = None):
        self.model = model
        self.base = url.rstrip("/")
        self.temperature = temperature
        self.max_tokens = max_tokens
        self.timeout = timeout
        self.options = options or {}

    def _body(self, messages: List[Message], stream: bool, opts: Dict[str, Any]) -> Dict[str, Any]:
        o = dict(self.options)
        t = opts.pop("temperature", self.temperature)
        if t is not None:
            o["temperature"] = t
        m = opts.pop("max_tokens", self.max_tokens)
        if m is not None:
            o["num_predict"] = m
        return {"model": self.model, "messages": messages, "stream": stream, "options": o, **opts}

    def chat(self, messages: List[Message], **opts: Any) -> str:
        body = self._body(messages, False, dict(opts))
        r = _retry(lambda: request("POST", self.base + "/api/chat", body=body, timeout=self.timeout))
        return ((r or {}).get("message") or {}).get("content", "")

    def stream(self, messages: List[Message], **opts: Any) -> Iterator[str]:
        body = self._body(messages, True, dict(opts))
        try:
            for line in stream_lines("POST", self.base + "/api/chat", body=body, timeout=self.timeout):
                try:
                    d = json.loads(line)
                except ValueError:
                    continue
                piece = (d.get("message") or {}).get("content")
                if piece:
                    yield piece
                if d.get("done"):
                    break
        except HTTPError as e:
            raise LLMError(f"{e.status}: {e.body[:500]}") from None

    def models(self) -> List[str]:
        r = _retry(lambda: request("GET", self.base + "/api/tags", timeout=30))
        return [m.get("name", "") for m in (r or {}).get("models", [])]

    def embed(self, text: str) -> List[float]:
        r = _retry(lambda: request("POST", self.base + "/api/embed", body={"model": self.model, "input": text}, timeout=self.timeout))
        return list((r or {}).get("embeddings", [[]])[0])


# ------------------------------------------------------------------------ Anthropic
class Anthropic(LLM):
    """The Anthropic Messages API. ``model`` is required; the key defaults to ``$ANTHROPIC_API_KEY``."""

    VERSION = "2023-06-01"

    def __init__(self, model: str, api_key: Optional[str] = None, *, url: str = "https://api.anthropic.com",
                 max_tokens: int = 1024, temperature: Optional[float] = None, timeout: float = 300):
        if not model:
            raise LLMError("Anthropic: pass model=…")
        self.model = model
        self.api_key = api_key or os.environ.get("ANTHROPIC_API_KEY", "")
        self.base = url.rstrip("/")
        self.max_tokens = max_tokens
        self.temperature = temperature
        self.timeout = timeout

    def _headers(self) -> Dict[str, str]:
        return {"x-api-key": self.api_key, "anthropic-version": self.VERSION}

    def _body(self, messages: List[Message], stream: bool, opts: Dict[str, Any]) -> Dict[str, Any]:
        system = "\n\n".join(m["content"] for m in messages if m["role"] == "system")
        msgs = [m for m in messages if m["role"] in ("user", "assistant")]
        body: Dict[str, Any] = {"model": self.model, "max_tokens": opts.pop("max_tokens", self.max_tokens),
                                "messages": msgs, "stream": stream}
        if system:
            body["system"] = system
        t = opts.pop("temperature", self.temperature)
        if t is not None:
            body["temperature"] = t
        body.update(opts)
        return body

    def chat(self, messages: List[Message], **opts: Any) -> str:
        body = self._body(messages, False, dict(opts))
        r = _retry(lambda: request("POST", self.base + "/v1/messages", headers=self._headers(), body=body, timeout=self.timeout))
        return "".join(b.get("text", "") for b in (r or {}).get("content", []) if b.get("type") == "text")

    def stream(self, messages: List[Message], **opts: Any) -> Iterator[str]:
        body = self._body(messages, True, dict(opts))
        try:
            for line in stream_lines("POST", self.base + "/v1/messages", headers=self._headers(), body=body, timeout=self.timeout):
                if not line.startswith("data:"):
                    continue
                try:
                    d = json.loads(line[5:].strip())
                except ValueError:
                    continue
                if d.get("type") == "content_block_delta":
                    piece = (d.get("delta") or {}).get("text")
                    if piece:
                        yield piece
                elif d.get("type") == "message_stop":
                    break
        except HTTPError as e:
            raise LLMError(f"{e.status}: {e.body[:500]}") from None


# ---------------------------------------------------------------------------- helpers
class ChatSession:
    """A conversation that remembers the last ``max_turns`` exchanges."""

    def __init__(self, llm: LLM, system: Optional[str] = None, max_turns: int = 12):
        self.llm = llm
        self.system = system
        self.max_turns = max_turns
        self.history: List[Message] = []

    def messages(self) -> List[Message]:
        head: List[Message] = [{"role": "system", "content": self.system}] if self.system else []
        h = self.history
        if h and h[-1]["role"] == "user":     # the pending question + the last max_turns exchanges
            return head + h[:-1][-2 * self.max_turns:] + h[-1:]
        return head + h[-2 * self.max_turns:]

    def ask(self, text: str, **opts: Any) -> str:
        self.history.append({"role": "user", "content": text})
        try:
            answer = self.llm.chat(self.messages(), **opts)
        except Exception:
            self.history.pop()
            raise
        self.history.append({"role": "assistant", "content": answer})
        del self.history[:-4 * self.max_turns]
        return answer

    def stream(self, text: str, **opts: Any) -> Iterator[str]:
        self.history.append({"role": "user", "content": text})
        parts: List[str] = []
        try:
            for p in self.llm.stream(self.messages(), **opts):
                parts.append(p)
                yield p
        finally:
            if parts:
                self.history.append({"role": "assistant", "content": "".join(parts)})
            else:
                self.history.pop()

    def reset(self) -> None:
        self.history.clear()


def connect(spec: Optional[str] = None) -> LLM:
    """Build a connector from a spec (see the module docstring) or ``$AQUA_LLM``."""
    spec = spec or os.environ.get("AQUA_LLM", "llamacpp")
    kind, _, rest = spec.partition(":")
    kind = kind.strip().lower()
    url = os.environ.get("AQUA_LLM_URL", "")
    model = os.environ.get("AQUA_LLM_MODEL", "")
    key = os.environ.get("AQUA_LLM_KEY", "")
    if kind in ("llamacpp", "llama.cpp", "llama"):
        return LlamaCpp(rest or url or "http://127.0.0.1:8080", model=model, api_key=key or None)
    if kind == "ollama":
        return Ollama(rest or model or "llama3.1", url=url or "http://127.0.0.1:11434")
    if kind == "openai":
        return OpenAICompatible(url or "https://api.openai.com/v1", model=rest or model,
                                api_key=key or os.environ.get("OPENAI_API_KEY"))
    if kind in ("openai-compatible", "compat", "oai"):
        # "openai-compatible:http://host:1234/v1:model" — split the model off the URL's tail.
        base, m = rest, model
        if rest.count(":") >= 2:
            head, _, tail = rest.rpartition(":")
            if not tail.isdigit() and "/" not in tail:
                base, m = head, tail
        return OpenAICompatible(base or url, model=m, api_key=key or None)
    if kind == "anthropic":
        return Anthropic(rest or model, api_key=key or None)
    raise LLMError(f"unknown model provider {kind!r} (llamacpp, ollama, openai, openai-compatible, anthropic)")
