"""Echo bot: repeats what you write.

    AQUA_SERVER=http://127.0.0.1:7777 AQUA_BOT_TOKEN=… python echo_bot.py
"""
from aquabot import Bot

bot = Bot()


@bot.command("start", help="greeting")
def start(ctx):
    ctx.reply(f"Hi, @{ctx.sender}! Write anything and I'll repeat it.")


@bot.default
def echo(ctx):
    ctx.reply(ctx.text)


bot.use_help()
bot.run()
