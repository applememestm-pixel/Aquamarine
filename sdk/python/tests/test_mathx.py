import math
import unittest

from aquabot.mathx import (CalcError, calc, det, derivative, factorize, fib, fmt, from_base, integrate, inverse,
                           is_prime, linreg, matmul, percentile, primes, solve, solve_linear, solve_quadratic, stats,
                           to_base)


class Calc(unittest.TestCase):
    def test_basics(self):
        cases = {
            "2+2*2": "6", "2^10": "1024", "2**3**2": "512", "(1+2)!": "6", "5!": "120", "10 % 3": "1",
            "7 // 2": "3", "0.1+0.2": "0.3", "√16 + 2√9": "10", "sqrt(-1)": "i", "sin(pi/2)": "1",
            "sind(30)": "0.5", "log(8, 2)": "3", "ln(e)": "1", "log10(1000)": "3", "comb(5,2)": "10",
            "gcd(12, 18)": "6", "lcm(4, 6)": "12", "mean([1,2,3,4])": "2.5", "max(1, 9, 3)": "9",
            "3 > 2": "1", "1 < 2 < 1": "0", "abs(-3)": "3", "round(2.567, 2)": "2.57", "6×7÷2": "21",
            "fib(10)": "55", "isprime(97)": "1", "-2^2": "-4", "2³": "8",
        }
        for e, want in cases.items():
            with self.subTest(e=e):
                self.assertEqual(fmt(calc(e)), want)

    def test_variables(self):
        self.assertEqual(calc("x^2 + y", {"x": 3, "y": 1}), 10)

    def test_rejects_unsafe(self):
        for e in ["__import__('os')", "(1).__class__", "open('x')", "[x for x in 1]", "lambda: 1", "'a'*3",
                  "2**999999999", "fact(100000)", "1/0", "unknown(1)", "foo", "9" * 3000, "True + 1"]:
            with self.subTest(e=e[:30]):
                with self.assertRaises(CalcError):
                    calc(e)

    def test_fmt(self):
        self.assertEqual(fmt(1e21), "1e+21")
        self.assertEqual(fmt(float("inf")), "∞")
        self.assertEqual(fmt(2 - 3j), "2 - 3i")
        self.assertTrue(fmt(2 ** 300).endswith("digits)"))


class Numeric(unittest.TestCase):
    def test_number_theory(self):
        self.assertEqual(factorize(360), [2, 2, 2, 3, 3, 5])
        self.assertEqual(factorize(600851475143), [71, 839, 1471, 6857])
        self.assertTrue(is_prime(2 ** 61 - 1))
        self.assertFalse(is_prime(561))
        self.assertEqual(primes(30), [2, 3, 5, 7, 11, 13, 17, 19, 23, 29])
        self.assertEqual(fib(90), 2880067194370816120)
        self.assertEqual(to_base(255, 2), "11111111")
        self.assertEqual(from_base("ff", 16), 255)

    def test_calculus(self):
        self.assertAlmostEqual(integrate(math.sin, 0, math.pi), 2, places=8)
        self.assertAlmostEqual(derivative(lambda x: x ** 3, 2), 12, places=6)
        self.assertAlmostEqual(solve(lambda x: x * x - 2, 0, 2), math.sqrt(2), places=9)
        self.assertEqual(solve_quadratic(1, -3, 2), (1.0, 2.0))
        self.assertEqual(solve_quadratic(1, 0, 1), (1j, -1j))
        self.assertEqual(solve_quadratic(0, 2, -4), (2.0,))

    def test_linear_algebra(self):
        self.assertEqual(det([[1, 2], [3, 4]]), -2)
        self.assertEqual(det([[2, 0, 0], [0, 3, 0], [0, 0, 4]]), 24)
        inv = inverse([[4, 7], [2, 6]])
        prod = matmul([[4, 7], [2, 6]], inv)
        for i in range(2):
            for j in range(2):
                self.assertAlmostEqual(prod[i][j], float(i == j))
        x = solve_linear([[2, 1], [1, 3]], [3, 5])
        self.assertAlmostEqual(x[0], 0.8)
        self.assertAlmostEqual(x[1], 1.4)
        with self.assertRaises(CalcError):
            inverse([[1, 2], [2, 4]])

    def test_stats(self):
        s = stats([2, 4, 4, 4, 5, 5, 7, 9])
        self.assertEqual(s["mean"], 5)
        self.assertEqual(s["median"], 4.5)
        self.assertEqual(percentile([1, 2, 3, 4], 50), 2.5)
        self.assertEqual(linreg([1, 2, 3], [3, 5, 7]), (2.0, 1.0, 1.0))


if __name__ == "__main__":
    unittest.main()
