"""AI bot on a local llama.cpp server.

1. Start the model:   llama-server -m qwen2.5-7b-instruct-q4_k_m.gguf --port 8080
2. Start the bot:     AQUA_BOT_TOKEN=… python llama_bot.py

Swap the model in one line: llm.Ollama("llama3.1"), llm.connect("openai:gpt-4o-mini"), …
"""
import logging

from aquabot import Bot, llm

logging.basicConfig(level=logging.INFO)

model = llm.LlamaCpp("http://127.0.0.1:8080")
if not model.health():
    raise SystemExit("llama-server isn't answering on :8080")

bot = Bot()
bot.use_help("I'm a local AI. Just write to me; /reset forgets the conversation.")
bot.use_calc()
bot.use_llm(model, system="You are a friendly assistant. Answer in the user's language, briefly.")


@bot.command("tokens", help="count tokens in a text")
def tokens(ctx):
    ctx.reply(f"{len(model.tokenize(ctx.args))} tokens")


bot.run()
