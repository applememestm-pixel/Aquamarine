"""Math bot: /calc, /factor, /solve, /stats, /prime."""
from aquabot import Bot, mathx

bot = Bot()
bot.use_help("Math helper.").use_calc()


@bot.command("factor", help="prime factors: /factor 360")
def factor(ctx):
    n = int(ctx.args)
    ctx.reply(f"{n} = " + " × ".join(map(str, mathx.factorize(n))))


@bot.command("solve", help="ax²+bx+c=0: /solve 1 -3 2")
def solve(ctx):
    a, b, c = (float(x) for x in ctx.args.split())
    ctx.reply("x = " + ", ".join(mathx.fmt(r) for r in mathx.solve_quadratic(a, b, c)))


@bot.command("stats", help="statistics: /stats 1 2 3 4")
def stats(ctx):
    s = mathx.stats(float(x) for x in ctx.args.replace(",", " ").split())
    ctx.reply("\n".join(f"{k}: {mathx.fmt(v)}" for k, v in s.items()))


@bot.command("prime", help="is it prime? /prime 97")
def prime(ctx):
    n = int(ctx.args)
    ctx.reply(f"{n} is {'prime' if mathx.is_prime(n) else 'not prime'}")


@bot.on_error
def oops(ctx, e):
    ctx.reply(f"Couldn't do that: {e}")


bot.run()
