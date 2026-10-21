# aquabot — боты для Aquamarine

Библиотека на Python для ботов мессенджера Aquamarine: каркас бота, подключение к нейросетям (llama.cpp, Ollama, любые OpenAI-совместимые серверы, Anthropic) и встроенная математика. Работает на одной стандартной библиотеке, Python ≥ 3.9.

*Python toolkit for Aquamarine messenger bots: bot framework, LLM connectors (llama.cpp, Ollama, OpenAI-compatible, Anthropic) and a safe math toolkit. Zero dependencies.*

## Установка

```bash
pip install "git+https://github.com/applememestm-pixel/Aquamarine#subdirectory=sdk/python"
# или, когда пакет выложен в PyPI:
pip install aquamarine-bot
```

## Бот с нейросетью за 6 строк

```python
from aquabot import Bot, llm

bot = Bot()                                   # $AQUA_SERVER, $AQUA_BOT_TOKEN
bot.use_help("Я ИИ-помощник.")                # /help
bot.use_calc()                                # /calc 2^10 + sqrt(16)
bot.use_llm(llm.LlamaCpp())                   # любой текст → модель; /ask, /reset
bot.run()
```

Или без кода:

```bash
export AQUA_SERVER=http://127.0.0.1:7777 AQUA_BOT_TOKEN=…   # токен из клиента: Настройки → Боты
llama-server -m model.gguf --port 8080                       # в другом терминале
aquabot run                                                  # готовый ИИ-бот
```

## Каркас бота

```python
from aquabot import Bot

bot = Bot(workers=8)

@bot.command("start", "hello", help="приветствие")
def start(ctx):
    ctx.reply(f"Привет, @{ctx.sender}!")

@bot.message(r"(?i)\bпинг\b")
def ping(ctx):
    ctx.reply("понг")

@bot.default                     # всё остальное
def other(ctx):
    ctx.state["n"] = ctx.state.get("n", 0) + 1     # состояние на пользователя
    ctx.reply(f"Сообщение №{ctx.state['n']}")

@bot.on_error
def oops(ctx, e):
    ctx.reply(f"Ошибка: {e}")

bot.use_help()
bot.run()
```

- Сообщения одного пользователя обрабатываются по порядку, разных — параллельно.
- Длинные ответы режутся на части по 4096 символов; ограничение 1 сообщение/с соблюдается.
- 429 и 5xx повторяются с паузой, сбои сети не роняют бота.
- `/stop` от пользователя очищает его состояние.
- `bot.send(user, text)` пишет первым тем, кто уже начал диалог.

Низкий уровень — `aquabot.Client`: `me()`, `updates(after, timeout)`, `send(to, text)`.

## Нейросети

Все подключения устроены одинаково: `chat(messages)`, `stream(messages)`, `ask(prompt, system=…)`.

```python
from aquabot import llm

m = llm.LlamaCpp("http://127.0.0.1:8080")             # llama.cpp: llama-server
m = llm.Ollama("llama3.1")                             # Ollama
m = llm.OpenAICompatible("http://127.0.0.1:1234/v1", "qwen2.5-7b")   # LM Studio, vLLM, LocalAI…
m = llm.OpenAICompatible("https://api.openai.com/v1", "gpt-4o-mini", api_key="…")
m = llm.Anthropic("<модель>", api_key="…")

print(m.ask("Почему небо голубое?", system="Отвечай коротко"))
for piece in m.stream([{"role": "user", "content": "Сочини хокку"}]):
    print(piece, end="", flush=True)
```

Диалог с памятью:

```python
chat = llm.ChatSession(m, system="Ты пират.", max_turns=12)
chat.ask("Как тебя зовут?")
chat.ask("А что я спросил?")     # модель видит историю
chat.reset()
```

Дополнительно: `m.models()`, `m.embed(text)` (OpenAI-совместимые, Ollama); у llama.cpp — `health()`, `tokenize(text)`, `complete(prompt)` (сырое продолжение без шаблона чата). Параметры: `temperature=`, `max_tokens=`, `timeout=`; любые другие поля — через `extra={…}` / `options={…}` или аргументы `chat(..., top_p=0.9)`.

Подключение из строки или окружения:

| `llm.connect(...)` / `$AQUA_LLM` | Что будет |
|---|---|
| `llamacpp` | `http://127.0.0.1:8080` |
| `llamacpp:http://gpu-box:8080` | llama.cpp на другой машине |
| `ollama:qwen2.5` | Ollama, модель qwen2.5 |
| `openai:gpt-4o-mini` | OpenAI, ключ из `$OPENAI_API_KEY` |
| `openai-compatible:http://host:1234/v1:model` | любой совместимый сервер |
| `anthropic:<модель>` | ключ из `$ANTHROPIC_API_KEY` |

`$AQUA_LLM_URL`, `$AQUA_LLM_MODEL`, `$AQUA_LLM_KEY` дополняют спецификацию.

## Математика

```python
from aquabot import calc, fmt, mathx

fmt(calc("2^10 + √16 * sin(pi/2)"))   # '1028'
fmt(calc("5! / 3!"))                   # '20'
fmt(calc("sqrt(-4)"))                  # '2i'
calc("x^2 + y", {"x": 3, "y": 1})      # 10
```

`calc` не использует `eval`: выражение разбирается через `ast`, разрешены только числа, операторы и функции из белого списка; огромные степени и факториалы отклоняются (`CalcError`).

- **Операторы:** `+ - * / // % ^ **`, сравнения, `5!`, `√`, `×`, `÷`, `²`, `³`.
- **Функции:**
  - корни, степени, логарифмы: `sqrt cbrt exp ln log(x, base) log2 log10 pow hypot`;
  - тригонометрия в радианах и градусах: `sin … atanh`, `sind cosd tand`, `deg rad`;
  - округление: `abs floor ceil round trunc sign`;
  - целые числа: `fact gamma gcd lcm comb perm isprime fib mod`;
  - агрегаты: `min max sum mean median stdev variance`;
  - комплексные числа: `re im conj phase`.
- **Константы:** `pi e tau phi i inf`.

Модуль `mathx` также умеет:

| Раздел | Функции |
|---|---|
| Теория чисел | `is_prime` (Миллер — Рабин), `factorize` (ρ-Полларда), `primes(n)` (решето), `fib`, `to_base` / `from_base` |
| Анализ | `derivative`, `integrate` (Симпсон), `solve(f, a, b)` (корень на отрезке), `solve_quadratic` |
| Матрицы | `matmul`, `transpose`, `det`, `inverse`, `solve_linear` |
| Статистика | `stats`, `percentile`, `linreg` (наклон, сдвиг, R²) |

## Командная строка

```bash
aquabot calc "2^64"
aquabot --llm ollama:llama3.1 ask "Привет!"
aquabot chat                      # диалог с моделью в терминале
aquabot models                    # модели на сервере
aquabot run [--no-chat]           # готовый бот: /help /calc /ask /reset
```

## Примеры

Готовые боты лежат в [`examples/`](examples):

- `echo_bot.py` — повторяет сообщения;
- `llama_bot.py` — ИИ на llama.cpp;
- `math_bot.py` — `/factor`, `/solve`, `/stats`, `/prime`.

## Безопасность

Переписка с ботом **не защищена сквозным шифрованием**: сервер ботов видит текст, и модель тоже его видит. Для приватных разговоров поднимите модель локально (llama.cpp или Ollama). Токен бота храните в переменной окружения, а не в коде. Подробности — в [docs/bots.md](../../docs/bots.md).

## Тесты

```bash
cd sdk/python/tests && PYTHONPATH=../src python3 -m unittest -v
```

Лицензия: MIT или Apache-2.0, на выбор.
