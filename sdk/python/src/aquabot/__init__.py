"""aquabot — bots for the Aquamarine messenger, with LLMs and math built in.

    from aquabot import Bot, llm

    bot = Bot()                                   # $AQUA_SERVER, $AQUA_BOT_TOKEN
    bot.use_help().use_calc().use_llm(llm.LlamaCpp())
    bot.run()

No third-party dependencies (Python ≥ 3.9 standard library only).
"""
from . import llm, mathx
from .bot import Bot, Context
from .client import BotError, Client, Forbidden, RateLimited, Unauthorized, Update, split_text
from .llm import LLM, Anthropic, ChatSession, LlamaCpp, LLMError, Ollama, OpenAICompatible, connect
from .mathx import CalcError, calc, fmt

__version__ = "0.2.0"

__all__ = [
    "Bot", "Context", "Client", "Update", "split_text",
    "BotError", "Unauthorized", "Forbidden", "RateLimited",
    "llm", "LLM", "LLMError", "ChatSession", "LlamaCpp", "OpenAICompatible", "Ollama", "Anthropic", "connect",
    "mathx", "calc", "fmt", "CalcError",
    "__version__",
]
