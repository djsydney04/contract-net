"""Run with python test_executor.py; compare optimized work with the answer key."""

import random
from statistics import median
from timeit import repeat

from contractnet import Task, run_task
from my_contractor import MyContractor
from verify import GOLDEN


def main():
    agent = MyContractor.__new__(MyContractor)
    cases = [(kind, dict(params)) for kind, params, _ in GOLDEN]
    rng = random.Random(370)
    for seed in range(30):
        lo = rng.randrange(-20, 20_000)
        cases.extend([
            ("prime_count", {"lo": lo, "hi": lo + rng.randrange(-2, 300)}),
            ("matmul_mod", {"seed": seed, "n": rng.randrange(0, 12),
                            "mod": rng.choice([1, 2, 97, 1_000_003, 2**70 + 33])}),
            ("sort_checksum", {"seed": seed, "n": rng.randrange(0, 1000)}),
            ("monte_carlo_pi", {"seed": seed, "samples": rng.randrange(0, 1000)}),
            ("hash_search", {"seed": f"seed:{seed}é", "threshold": 2**24 + seed}),
        ])
    cases.extend(("prime_count", {"lo": lo, "hi": hi}) for lo, hi in
                 [(-10, 3), (2, 2), (2, 3), (3, 4), (47, 50), (2, 1_000_010)])
    cases.extend(("hash_search", {"seed": "edge", "threshold": threshold})
                 for threshold in [2**32 - 1, 2**32, 2**32 + 1])
    for kind, params in cases:
        task = Task(0, kind, params, 999, 999, 0, 1)
        assert agent.execute(task) == run_task(kind, params), (kind, params)
    try:
        agent.execute(Task(0, "hash_search", {"seed": 0, "threshold": 0}, 1, 1, 0, 1))
    except ValueError:
        pass
    else:
        raise AssertionError("invalid threshold must fail instead of hanging")
    print(f"Passed {len(cases)} reference comparisons and invalid-threshold check.")
    for kind, params, _ in GOLDEN:
        task = Task(0, kind, dict(params), 999, 999, 0, 1)
        reference = median(repeat(lambda: run_task(kind, params), number=1, repeat=7))
        optimized = median(repeat(lambda: agent.execute(task), number=1, repeat=7))
        print(f"{kind:16} {reference * 1000:8.3f} -> {optimized * 1000:8.3f} ms"
              f" ({reference / optimized:.2f}x faster)")


if __name__ == "__main__":
    main()
