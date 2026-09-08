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

from contractnet import Bid, Contractor, Task


class MyContractor(Contractor):
    """A baseline bidder. Beating it is the assignment."""

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

    # Extra credit: override `execute` with a faster implementation. It must
    # return exactly the same integer as the reference, or the manager scores
    # it as a wrong answer and fines you.
    #
    # def execute(self, task: Task) -> int:
    #     if task.task_type == "matmul_mod":
    #         ...  # numpy with int64 and careful modular reduction
    #     return super().execute(task)


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
