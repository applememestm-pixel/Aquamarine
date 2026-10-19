"""Built-in math: a safe calculator and the everyday numeric tools.

    >>> calc("2^10 + sqrt(16) * sin(pi/2)")
    1028.0
    >>> fmt(calc("1/3"))
    '0.333333333333'

Nothing here uses ``eval``: expressions are parsed with :mod:`ast` and only
numbers, arithmetic and the whitelisted functions/constants are allowed.
"""
from __future__ import annotations

import ast
import cmath
import math
import operator
import re
import statistics
from fractions import Fraction
from typing import Callable, Dict, Iterable, List, Optional, Sequence, Tuple, Union

Number = Union[int, float, complex]


class CalcError(ValueError):
    """Bad or unsafe expression."""


# ------------------------------------------------------------------ number theory
def is_prime(n: int) -> bool:
    """Deterministic Miller–Rabin for n < 3.3·10^24, probabilistic beyond."""
    if n < 2:
        return False
    small = (2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37)
    for p in small:
        if n % p == 0:
            return n == p
    d, s = n - 1, 0
    while d % 2 == 0:
        d //= 2
        s += 1
    for a in small:
        x = pow(a, d, n)
        if x in (1, n - 1):
            continue
        for _ in range(s - 1):
            x = x * x % n
            if x == n - 1:
                break
        else:
            return False
    return True


def _rho(n: int) -> int:
    if n % 2 == 0:
        return 2
    c = 1
    while True:
        x = y = 2
        d = 1
        f = lambda v: (v * v + c) % n  # noqa: E731
        while d == 1:
            x = f(x)
            y = f(f(y))
            d = math.gcd(abs(x - y), n)
        if d != n:
            return d
        c += 1


def factorize(n: int) -> List[int]:
    """Prime factors with multiplicity, ascending: ``factorize(360) == [2, 2, 2, 3, 3, 5]``."""
    n = int(n)
    if n < 1:
        raise CalcError("factorize needs a positive integer")
    out: List[int] = []
    for p in (2, 3, 5):
        while n % p == 0:
            out.append(p)
            n //= p
    stack = [n] if n > 1 else []
    while stack:
        m = stack.pop()
        if m == 1:
            continue
        if is_prime(m):
            out.append(m)
            continue
        d = _rho(m)
        stack += [d, m // d]
    return sorted(out)


def primes(limit: int) -> List[int]:
    """All primes ≤ limit (sieve)."""
    if limit < 2:
        return []
    if limit > 50_000_000:
        raise CalcError("limit too large")
    sieve = bytearray([1]) * (limit + 1)
    sieve[0:2] = b"\x00\x00"
    for i in range(2, int(limit ** 0.5) + 1):
        if sieve[i]:
            sieve[i * i::i] = bytearray(len(range(i * i, limit + 1, i)))
    return [i for i, v in enumerate(sieve) if v]


def fib(n: int) -> int:
    """n-th Fibonacci number (fast doubling)."""
    if n < 0:
        raise CalcError("fib needs n ≥ 0")
    if n > 1_000_000:
        raise CalcError("n too large")

    def go(k: int) -> Tuple[int, int]:
        if k == 0:
            return 0, 1
        a, b = go(k >> 1)
        c = a * (2 * b - a)
        d = a * a + b * b
        return (d, c + d) if k & 1 else (c, d)

    return go(int(n))[0]


def to_base(n: int, base: int) -> str:
    """``to_base(255, 16) == 'ff'``; bases 2–36."""
    if not 2 <= base <= 36:
        raise CalcError("base must be 2..36")
    n = int(n)
    if n == 0:
        return "0"
    digits = "0123456789abcdefghijklmnopqrstuvwxyz"
    sign, n = ("-", -n) if n < 0 else ("", n)
    out = []
    while n:
        n, r = divmod(n, base)
        out.append(digits[r])
    return sign + "".join(reversed(out))


def from_base(s: str, base: int) -> int:
    return int(str(s).strip(), base)


# ------------------------------------------------------------------ calculus / roots
def derivative(f: Callable[[float], float], x: float, h: float = 1e-5) -> float:
    """Central five-point difference."""
    return (-f(x + 2 * h) + 8 * f(x + h) - 8 * f(x - h) + f(x - 2 * h)) / (12 * h)


def integrate(f: Callable[[float], float], a: float, b: float, n: int = 1000) -> float:
    """Composite Simpson's rule (n rounded up to even)."""
    if n % 2:
        n += 1
    h = (b - a) / n
    s = f(a) + f(b)
    for i in range(1, n):
        s += f(a + i * h) * (4 if i % 2 else 2)
    return s * h / 3


def solve(f: Callable[[float], float], a: float, b: float, tol: float = 1e-12, maxiter: int = 200) -> float:
    """A root of f in [a, b] (f(a) and f(b) must differ in sign): bisection + secant."""
    fa, fb = f(a), f(b)
    if fa == 0:
        return a
    if fb == 0:
        return b
    if fa * fb > 0:
        raise CalcError("f(a) and f(b) have the same sign")
    for _ in range(maxiter):
        m = b - fb * (b - a) / (fb - fa) if fb != fa else (a + b) / 2
        if not (min(a, b) < m < max(a, b)):
            m = (a + b) / 2
        fm = f(m)
        if abs(fm) < tol or abs(b - a) < tol:
            return m
        if fa * fm < 0:
            b, fb = m, fm
        else:
            a, fa = m, fm
        mid = (a + b) / 2
        fmid = f(mid)
        if fa * fmid < 0:
            b, fb = mid, fmid
        else:
            a, fa = mid, fmid
    return (a + b) / 2


def solve_quadratic(a: float, b: float, c: float) -> Tuple[Number, ...]:
    """Roots of ax² + bx + c (complex when needed; linear when a == 0)."""
    if a == 0:
        if b == 0:
            raise CalcError("no unknown left")
        return (-c / b,)
    d = b * b - 4 * a * c
    if d >= 0:
        r = math.sqrt(d)
        q = -0.5 * (b + math.copysign(r, b))
        x1 = q / a
        x2 = c / q if q != 0 else -b / (2 * a) - x1
        return tuple(sorted((x1, x2))) if d > 0 else (x1,)
    r = cmath.sqrt(d)
    return ((-b + r) / (2 * a), (-b - r) / (2 * a))


# -------------------------------------------------------------------------- matrices
Matrix = List[List[float]]


def _check(m: Sequence[Sequence[float]]) -> Tuple[int, int]:
    rows = len(m)
    cols = len(m[0]) if rows else 0
    if any(len(r) != cols for r in m):
        raise CalcError("ragged matrix")
    return rows, cols


def matmul(a: Sequence[Sequence[float]], b: Sequence[Sequence[float]]) -> Matrix:
    ra, ca = _check(a)
    rb, cb = _check(b)
    if ca != rb:
        raise CalcError(f"can't multiply {ra}×{ca} by {rb}×{cb}")
    return [[sum(a[i][k] * b[k][j] for k in range(ca)) for j in range(cb)] for i in range(ra)]


def transpose(m: Sequence[Sequence[float]]) -> Matrix:
    return [list(r) for r in zip(*m)]


def _lu(m: Sequence[Sequence[float]]):
    n, c = _check(m)
    if n != c:
        raise CalcError("matrix must be square")
    a = [[Fraction(x) if isinstance(x, int) else x for x in row] for row in m]
    return n, a


def det(m: Sequence[Sequence[float]]) -> float:
    """Determinant (Gaussian elimination with partial pivoting)."""
    n, a = _lu(m)
    sign: float = 1
    d: float = 1
    for i in range(n):
        p = max(range(i, n), key=lambda r: abs(a[r][i]))
        if a[p][i] == 0:
            return 0
        if p != i:
            a[i], a[p] = a[p], a[i]
            sign = -sign
        d *= a[i][i]
        for r in range(i + 1, n):
            f = a[r][i] / a[i][i]
            for k in range(i, n):
                a[r][k] -= f * a[i][k]
    res = sign * d
    return int(res) if isinstance(res, Fraction) and res.denominator == 1 else float(res)


def inverse(m: Sequence[Sequence[float]]) -> Matrix:
    """Inverse (Gauss–Jordan)."""
    n, a = _lu(m)
    aug = [row + [Fraction(int(i == j)) for j in range(n)] for i, row in enumerate(a)]
    for i in range(n):
        p = max(range(i, n), key=lambda r: abs(aug[r][i]))
        if aug[p][i] == 0:
            raise CalcError("matrix is singular")
        aug[i], aug[p] = aug[p], aug[i]
        piv = aug[i][i]
        aug[i] = [x / piv for x in aug[i]]
        for r in range(n):
            if r != i and aug[r][i] != 0:
                f = aug[r][i]
                aug[r] = [x - f * y for x, y in zip(aug[r], aug[i])]
    return [[float(x) for x in row[n:]] for row in aug]


def solve_linear(a: Sequence[Sequence[float]], b: Sequence[float]) -> List[float]:
    """x for A·x = b."""
    inv = inverse(a)
    return [sum(inv[i][j] * b[j] for j in range(len(b))) for i in range(len(inv))]


# -------------------------------------------------------------------------- statistics
def stats(data: Iterable[float]) -> Dict[str, float]:
    xs = [float(x) for x in data]
    if not xs:
        raise CalcError("no data")
    out = {"n": len(xs), "sum": math.fsum(xs), "min": min(xs), "max": max(xs),
           "mean": statistics.fmean(xs), "median": statistics.median(xs)}
    if len(xs) > 1:
        out["stdev"] = statistics.stdev(xs)
        out["variance"] = statistics.variance(xs)
    return out


def percentile(data: Iterable[float], p: float) -> float:
    """Linear interpolation between closest ranks; p in [0, 100]."""
    xs = sorted(float(x) for x in data)
    if not xs:
        raise CalcError("no data")
    if not 0 <= p <= 100:
        raise CalcError("p must be 0..100")
    k = (len(xs) - 1) * p / 100
    lo = int(math.floor(k))
    hi = min(lo + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


def linreg(xs: Sequence[float], ys: Sequence[float]) -> Tuple[float, float, float]:
    """Least squares y = slope·x + intercept → (slope, intercept, r²)."""
    if len(xs) != len(ys) or len(xs) < 2:
        raise CalcError("need two equal-length series of ≥ 2 points")
    mx, my = statistics.fmean(xs), statistics.fmean(ys)
    sxx = sum((x - mx) ** 2 for x in xs)
    if sxx == 0:
        raise CalcError("all x are equal")
    sxy = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    syy = sum((y - my) ** 2 for y in ys)
    slope = sxy / sxx
    r2 = (sxy * sxy) / (sxx * syy) if syy else 1.0
    return slope, my - slope * mx, r2


# ------------------------------------------------------------------- safe calculator
MAX_POW = 10_000          # largest allowed integer exponent
MAX_INT_BITS = 100_000    # refuse integers bigger than this
MAX_FACT = 5000


def _fact(n: Number) -> Number:
    if isinstance(n, float) and n.is_integer():
        n = int(n)
    if isinstance(n, int):
        if n < 0:
            raise CalcError("factorial of a negative number")
        if n > MAX_FACT:
            raise CalcError("factorial too large")
        return math.factorial(n)
    return math.gamma(float(n) + 1)


def _int_arg(f: Callable[..., int]) -> Callable[..., int]:
    def g(*args: Number) -> int:
        conv = []
        for a in args:
            if isinstance(a, float) and a.is_integer():
                a = int(a)
            if not isinstance(a, int):
                raise CalcError(f"{f.__name__} needs integers")
            conv.append(a)
        return f(*conv)
    g.__name__ = f.__name__
    return g


def _log(x: Number, base: Optional[Number] = None) -> Number:
    if base is None:
        return cmath.log(x) if isinstance(x, complex) or x < 0 else math.log(x)
    return math.log(x, base)


def _sqrt(x: Number) -> Number:
    return cmath.sqrt(x) if isinstance(x, complex) or x < 0 else math.sqrt(x)


def _seq(f: Callable[[List[float]], float]) -> Callable[..., float]:
    def g(*args: Number) -> float:
        if len(args) == 1 and isinstance(args[0], (list, tuple)):
            args = tuple(args[0])
        if not args:
            raise CalcError("needs at least one value")
        return f(list(args))  # type: ignore[arg-type]
    return g


FUNCTIONS: Dict[str, Callable[..., Number]] = {
    # roots / powers / logs
    "sqrt": _sqrt, "cbrt": lambda x: math.copysign(abs(x) ** (1 / 3), x), "exp": math.exp,
    "ln": _log, "log": _log, "log2": math.log2, "log10": math.log10, "pow": lambda a, b: _pow(a, b),
    "hypot": math.hypot,
    # trig (radians) + degree helpers
    "sin": math.sin, "cos": math.cos, "tan": math.tan, "asin": math.asin, "acos": math.acos,
    "atan": math.atan, "atan2": math.atan2, "sinh": math.sinh, "cosh": math.cosh, "tanh": math.tanh,
    "asinh": math.asinh, "acosh": math.acosh, "atanh": math.atanh,
    "sind": lambda d: math.sin(math.radians(d)), "cosd": lambda d: math.cos(math.radians(d)),
    "tand": lambda d: math.tan(math.radians(d)),
    "deg": math.degrees, "rad": math.radians,
    # rounding
    "abs": abs, "floor": math.floor, "ceil": math.ceil, "round": lambda x, n=0: round(x, int(n)) if n else round(x),
    "trunc": math.trunc, "sign": lambda x: (x > 0) - (x < 0),
    # integers
    "fact": _fact, "factorial": _fact, "gamma": math.gamma, "gcd": _int_arg(math.gcd),
    "lcm": _int_arg(lambda *a: math.lcm(*a) if hasattr(math, "lcm") else _lcm(*a)),
    "comb": _int_arg(math.comb), "ncr": _int_arg(math.comb), "perm": _int_arg(math.perm), "npr": _int_arg(math.perm),
    "isprime": lambda n: int(is_prime(int(n))), "fib": lambda n: fib(int(n)),
    "mod": lambda a, b: a % b,
    # aggregates
    "min": _seq(min), "max": _seq(max), "sum": _seq(math.fsum), "mean": _seq(statistics.fmean),
    "avg": _seq(statistics.fmean), "median": _seq(statistics.median),
    "stdev": _seq(statistics.stdev), "variance": _seq(statistics.variance),
    # complex
    "re": lambda z: complex(z).real, "im": lambda z: complex(z).imag, "conj": lambda z: complex(z).conjugate(),
    "phase": cmath.phase,
}

CONSTANTS: Dict[str, Number] = {
    "pi": math.pi, "π": math.pi, "e": math.e, "tau": math.tau, "τ": math.tau,
    "phi": (1 + math.sqrt(5)) / 2, "φ": (1 + math.sqrt(5)) / 2, "inf": math.inf, "i": 1j, "j": 1j,
}


def _lcm(*a: int) -> int:
    out = 1
    for x in a:
        out = abs(out * x) // math.gcd(out, x) if x else 0
    return out


def _pow(a: Number, b: Number) -> Number:
    if isinstance(b, (int, float)) and abs(b) > MAX_POW and isinstance(a, int) and abs(a) > 1:
        raise CalcError("power too large")
    if isinstance(a, int) and isinstance(b, int) and b > 0:
        if a and b * max(1, abs(a).bit_length()) > MAX_INT_BITS:
            raise CalcError("result too large")
    if isinstance(a, (int, float)) and a < 0 and isinstance(b, float) and not b.is_integer():
        return complex(a) ** b
    try:
        return a ** b
    except OverflowError:
        raise CalcError("result too large") from None


def _mul(a: Number, b: Number) -> Number:
    if isinstance(a, int) and isinstance(b, int) and a.bit_length() + b.bit_length() > MAX_INT_BITS:
        raise CalcError("result too large")
    return a * b


def _div(a: Number, b: Number) -> Number:
    if b == 0:
        raise CalcError("division by zero")
    return a / b


def _fdiv(a: Number, b: Number) -> Number:
    if b == 0:
        raise CalcError("division by zero")
    return a // b  # type: ignore[operator]


def _modop(a: Number, b: Number) -> Number:
    if b == 0:
        raise CalcError("division by zero")
    return a % b  # type: ignore[operator]


_BIN = {ast.Add: operator.add, ast.Sub: operator.sub, ast.Mult: _mul, ast.Div: _div, ast.FloorDiv: _fdiv,
        ast.Mod: _modop, ast.Pow: _pow, ast.BitXor: _pow}
_UN = {ast.UAdd: operator.pos, ast.USub: operator.neg}
_CMP = {ast.Lt: operator.lt, ast.LtE: operator.le, ast.Gt: operator.gt, ast.GtE: operator.ge,
        ast.Eq: operator.eq, ast.NotEq: operator.ne}


def _roots(s: str) -> str:
    """√2 → sqrt(2), √x → sqrt(x), √(…) → sqrt(…), √f(…) → sqrt(f(…))."""
    while "√" in s:
        i = s.index("√")
        j = i + 1
        while j < len(s) and s[j] == " ":
            j += 1
        k = j
        while k < len(s) and (s[k].isalnum() or s[k] in "._"):
            k += 1
        if k < len(s) and s[k] == "(":
            depth = 0
            while k < len(s):
                depth += s[k] == "("
                depth -= s[k] == ")"
                k += 1
                if depth == 0:
                    break
        if k == j:
            raise CalcError("'√' without a number")
        s = s[:i] + "sqrt(" + s[j:k] + ")" + s[k:]
    return s


def _prepare(expr: str) -> str:
    s = expr.strip()
    for a, b in (("×", "*"), ("·", "*"), ("÷", "/"), ("−", "-"), ("²", "**2"), ("³", "**3"),
                 (",", ","), ("**", "\x00")):
        s = s.replace(a, b)
    s = s.replace("^", "**").replace("\x00", "**")
    s = _roots(re.sub(r"(\d|\))\s*√", r"\1*√", s))
    # postfix factorial: 5! → fact(5), (n+1)! → fact((n+1))
    out: List[str] = []
    i = 0
    while i < len(s):
        ch = s[i]
        if ch == "!" and (i + 1 >= len(s) or s[i + 1] != "="):
            # find the operand to the left
            j = len(out) - 1
            while j >= 0 and out[j] == " ":
                j -= 1
            if j < 0:
                raise CalcError("'!' without a number")
            if out[j] == ")":
                depth = 0
                k = j
                while k >= 0:
                    if out[k] == ")":
                        depth += 1
                    elif out[k] == "(":
                        depth -= 1
                        if depth == 0:
                            break
                    k -= 1
                if k < 0:
                    raise CalcError("unbalanced parentheses")
                start = k
                while start > 0 and (out[start - 1].isalnum() or out[start - 1] == "_"):
                    start -= 1
            else:
                start = j
                while start > 0 and (out[start - 1].isalnum() or out[start - 1] in "._"):
                    start -= 1
            operand = "".join(out[start:j + 1])
            out[start:] = list(f"fact({operand})")
            i += 1
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def calc(expr: str, variables: Optional[Dict[str, Number]] = None) -> Number:
    """Evaluate a math expression safely.

    Supports ``+ - * / // % ^ **``, comparisons, ``5!``, ``√``, ``×``/``÷``,
    the functions in :data:`FUNCTIONS`, the constants in :data:`CONSTANTS`
    and your own ``variables``. Lists ``[1, 2, 3]`` work as arguments
    (``mean([1,2,3])``).
    """
    if len(expr) > 2000:
        raise CalcError("expression too long")
    src = _prepare(expr)
    try:
        tree = ast.parse(src, mode="eval")
    except SyntaxError:
        raise CalcError(f"can't parse: {expr}") from None
    names: Dict[str, Number] = dict(CONSTANTS)
    if variables:
        names.update(variables)

    def ev(n: ast.AST) -> Number:
        if isinstance(n, ast.Expression):
            return ev(n.body)
        if isinstance(n, ast.Constant):
            if isinstance(n.value, bool) or not isinstance(n.value, (int, float, complex)):
                raise CalcError("only numbers are allowed")
            return n.value
        if isinstance(n, ast.BinOp) and type(n.op) in _BIN:
            return _BIN[type(n.op)](ev(n.left), ev(n.right))
        if isinstance(n, ast.UnaryOp) and type(n.op) in _UN:
            return _UN[type(n.op)](ev(n.operand))
        if isinstance(n, ast.Compare) and all(type(o) in _CMP for o in n.ops):
            left = ev(n.left)
            for op, right_node in zip(n.ops, n.comparators):
                right = ev(right_node)
                if not _CMP[type(op)](left, right):
                    return 0
                left = right
            return 1
        if isinstance(n, ast.Name):
            if n.id in names:
                return names[n.id]
            raise CalcError(f"unknown name: {n.id}")
        if isinstance(n, (ast.List, ast.Tuple)):
            return [ev(e) for e in n.elts]  # type: ignore[return-value]
        if isinstance(n, ast.Call) and isinstance(n.func, ast.Name) and not n.keywords:
            fn = FUNCTIONS.get(n.func.id)
            if fn is None:
                raise CalcError(f"unknown function: {n.func.id}")
            args = [ev(a) for a in n.args]
            try:
                return fn(*args)
            except CalcError:
                raise
            except (ValueError, TypeError, ZeroDivisionError, OverflowError, statistics.StatisticsError) as e:
                raise CalcError(f"{n.func.id}: {e}") from None
        raise CalcError(f"not allowed: {ast.dump(n)[:40]}")

    try:
        v = ev(tree)
    except RecursionError:
        raise CalcError("expression too deep") from None
    if isinstance(v, list):
        raise CalcError("the result is a list, not a number")
    return v


def fmt(v: Number, digits: int = 12) -> str:
    """Human-friendly number: ``fmt(0.1 + 0.2) == '0.3'``, ``fmt(1e21) == '1e+21'``."""
    if isinstance(v, bool):
        return str(int(v))
    if isinstance(v, int):
        s = str(v)
        return s if len(s) <= 60 else f"{s[:20]}…{s[-10:]} ({len(s)} digits)"
    if isinstance(v, complex):
        if abs(v.imag) < 1e-15:
            return fmt(v.real, digits)
        re, im = fmt(v.real, digits), fmt(abs(v.imag), digits)
        im = "" if im == "1" else im
        sign = "-" if v.imag < 0 else "+"
        if re == "0":
            return f"{'-' if sign == '-' else ''}{im}i"
        return f"{re} {sign} {im}i"
    if math.isnan(v):
        return "NaN"
    if math.isinf(v):
        return "∞" if v > 0 else "-∞"
    if v.is_integer() and abs(v) < 1e15:
        return str(int(v))
    s = f"{v:.{digits}g}"
    if "e" not in s and "." in s:
        s = s.rstrip("0").rstrip(".")
    return s
