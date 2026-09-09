"""
CPSC 370, Assignment 1: your Contract Net contractor.

This file is yours. It runs as-is with a deliberately mediocre strategy, so
start by running it against the practice room, watch it lose money, and then
make it better.

    python my_contractor.py --name Team_07 --url wss://contractnet.example.com/agent

Everything you need is on `self`:

    self.estimate(task)   predicted seconds for this task on this machine
    self.queue_seconds    seconds of work you are already committed to
    self.rules            the manager's published scoring rules
    self.history          every Settlement you have received so far
    self.profit           your running profit

And on `task`:

    task.task_type        "monte_carlo_pi", "prime_count", "hash_search",
                          "sort_checksum", or "matmul_mod"
    task.params           the parameters for this instance
    task.budget           the most the manager will pay; higher bids are void
    task.deadline_s       seconds you get, measured from the moment you win
    task.work             a proportional estimate of how much work this is
"""

from __future__ import annotations

import argparse
import hashlib
import math
import random

from contractnet import Bid, Contractor, Task
from contractnet.tasks import CHECKSUM_MOD


class MyContractor(Contractor):
    """Baseline bidding with optimized, reference-compatible execution."""

    def on_cfp(self, task: Task) -> Bid | None:
        # How long will this take me, including the work already in my queue?
        compute_seconds = self.estimate(task)
        finish_in = self.queue_seconds + compute_seconds

        # Never promise something the clock says you cannot deliver.
        if finish_in > task.deadline_s:
            return None

        # What the job actually costs me to run.
        cost = compute_seconds * self.rules.cost_rate

        # TODO(you): this asks for a flat 60% markup and ignores everything
        # interesting. Consider:
        #
        #   * The auction is scored as `price + time_weight * est_seconds`
        #     under the default best_value policy. A fast machine can charge
        #     MORE and still win. Are you leaving money on the table?
        #   * A bid above `task.budget` is thrown out. So is an est_seconds
        #     above `task.deadline_s`. Check both before you send.
        #   * `hash_search` runtime is a coin flip: the expected number of
        #     attempts is 2**32 / threshold, but any single instance can take
        #     two or three times that, or finish almost at once. What premium
        #     should you charge for that risk?
        #   * Look at `self.history`. If your estimates keep coming in low,
        #     your calibration is optimistic. Correct for it.
        #   * Losing an auction costs nothing. Winning one you then blow the
        #     deadline on costs you `task.budget * self.rules.penalty_rate`.
        #     Note that is off the BUDGET, not off your bid, so underbidding
        #     does not shrink your downside.
        price = cost * 1.6

        if price > task.budget:
            return None

        # Quote the honest finish time: under best_value scoring, padding your
        # estimate to look safe also makes your bid look worse.
        return Bid(price=price, est_seconds=finish_in)

    def execute(self, task: Task) -> int:
        """Compute the reference integer with less work and no extra dependencies."""
        params = task.params
        if task.task_type == "prime_count":
            lo, hi = max(2, int(params["lo"])), int(params["hi"])
            if hi <= lo:
                return 0
            limit = math.isqrt(hi - 1)
            # ponytail: O(sqrt(hi)) base sieve; use segmented base primes for huge bounds.
            primes = bytearray(b"\x01") * (limit + 1)
            primes[:2] = b"\x00\x00"
            for p in range(2, math.isqrt(limit) + 1):
                if primes[p]:
                    primes[p * p::p] = b"\x00" * ((limit - p * p) // p + 1)
            divisors = [p for p in range(2, limit + 1) if primes[p]]
            total = 0
            # Bound interval storage even for wide ranges.
            for start in range(lo, hi, 1_000_000):
                stop = min(start + 1_000_000, hi)
                candidates = bytearray(b"\x01") * (stop - start)
                for p in divisors:
                    first = max(p * p, ((start + p - 1) // p) * p)
                    if first < stop:
                        candidates[first - start::p] = b"\x00" * ((stop - 1 - first) // p + 1)
                total += candidates.count(1)
            return total

        if task.task_type == "matmul_mod":
            n, mod = int(params["n"]), int(params["mod"])
            randrange = random.Random(int(params["seed"])).randrange
            # sum(A @ B) = sum_k(column_sum(A, k) * row_sum(B, k)).
            # Preserve the reference's random draw order; use O(n) storage.
            columns = [0] * n
            for _ in range(n):
                for k in range(n):
                    columns[k] += randrange(mod)
            total = 0
            for column in columns:
                total += column * sum(randrange(mod) for _ in range(n))
            return total % mod

        if task.task_type == "sort_checksum":
            getrandbits = random.Random(int(params["seed"])).getrandbits
            values = []
            for _ in range(int(params["n"])):
                # Match randrange(2**31)'s 32-bit rejection sampling exactly.
                value = getrandbits(32)
                while value >= 2**31:
                    value = getrandbits(32)
                values.append(value)
            values.sort()
            return sum(i * value for i, value in enumerate(values, 1)) % CHECKSUM_MOD

        if task.task_type == "hash_search":
            threshold = int(params["threshold"])
            if threshold <= 0:
                raise ValueError("hash threshold must be positive")
            if threshold >= 2**32:
                return 0
            target = threshold.to_bytes(4, "big")
            copy_hash = hashlib.sha256(f"{params['seed']}:".encode()).copy
            nonce = 0
            while True:
                digest = copy_hash()
                digest.update(str(nonce).encode())
                if digest.digest()[:4] < target:
                    return nonce
                nonce += 1

        if task.task_type == "monte_carlo_pi":
            draw = random.Random(int(params["seed"])).random
            inside = 0
            for _ in range(int(params["samples"])):
                x, y = draw(), draw()
                if x * x + y * y <= 1.0:
                    inside += 1
            return inside

        return super().execute(task)


def main() -> None:
    parser = argparse.ArgumentParser(description="CPSC 370 Contract Net contractor")
    parser.add_argument("--name", required=True, help="your team name, e.g. Team_07")
    parser.add_argument("--url", required=True, help="wss://.../agent")
    parser.add_argument("--token", default=None, help="class token, if the room requires one")
    parser.add_argument("--machine", default=None, help="label shown on the leaderboard")
    args = parser.parse_args()

    MyContractor(
        name=args.name,
        url=args.url,
        token=args.token,
        machine=args.machine,
    ).run()


if __name__ == "__main__":
    main()
