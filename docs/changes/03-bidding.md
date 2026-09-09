# How the bidder changed

[Back to the guide](../README.md)

## A correct answer could still lose money

The old bidder mainly priced the time spent calculating the answer. The manager
charges for the elapsed time from awarding the task until receiving the result.
That can also include delivery, waiting, and other overhead.

For example, suppose computation takes 1 ms and additional elapsed time takes
50 ms. At a cost of one dollar per second, the old 60% markup would ask for
$0.0016 while the total cost would be about $0.051. A correct answer would lose
roughly five cents. This is an illustration, not another measured auction.

The repeated small losses in the practice logs made this gap visible. The
updated bidder estimates the broader cost before deciding whether to bid.

## It learns from completed work

For accepted timing records, the client saves the task identity, its original
estimate, actual local calculation time, local queue time, manager-reported
elapsed time, and settlement cost and profit. Correct and late settlements
can supply observations when the necessary timings are available and valid.

Only the latest 512 valid observations are kept. The default file is under
`data/runtime/` and is chosen using the manager URL and team name; the room is
part of that URL. The file contains learned observations, not the registration
token or cached task answers.

`--state PATH` chooses a different file. `--no-state` keeps learning only in the
running process. Saves write a temporary file and then rename it into place,
which avoids exposing a half-written replacement as the normal state file.

The model recognizes a repeated task by its type and full input values, rather
than by its task number. For most task types, it can also use timing experience
from other inputs of the same type. A hash search is treated more carefully:
another seed can require a very different number of attempts, so exact-input
history is used for that task's learned search difficulty.

## It keeps an expected time and a more cautious reserve

The expected time describes a normal calculation based on the middle observed
timing ratio, with a small margin. A separate reserve uses a slower observed
percentile and an additional margin. The reserve helps decide whether a task
fits its deadline and whether its price covers a cautious cost estimate.

Without matching experience, ordinary tasks receive a calculation reserve above
the startup estimate. An unseen hash input receives a larger reserve because
the number of guesses can vary substantially. After exact inputs have been
executed, the model can use their observed difficulty.

Delivery begins with an allowance of 55 ms: a 50 ms starting assumption plus a
5 ms margin. After observations arrive, the allowance uses the observed p95
overhead plus 5 ms, with a small minimum. P95 means a value at or above 95% of
the observations in the saved sample; it is not a guaranteed upper bound.

To estimate overhead, the client starts with the larger of the manager's time
and the elapsed time implied by its bill, then subtracts local computation and
local queue time. It pools those overhead observations across task types.
Computation and queue estimates remain separate. This is an allowance for
unexplained elapsed time, not a direct measurement of network travel alone.

## It prices against visible competition

The common `best_value` rule combines price and estimated completion time.
The manager adds the price to the estimated seconds multiplied by its time
weight. A lower combined score is better.

The bidder calculates a price that comes below the best suitable visible
competitor's score by a 2% margin. It also stays below 98% of the task budget.
This can leave much more room for profit than applying a small markup to a
very fast local calculation.

Before submitting that price, the client checks its minimum acceptable price.
That minimum covers a cautious elapsed-cost estimate, a 20% margin, and a small
extra amount. If the available price cannot cover that floor, it refuses the
task. It also refuses work whose reserved finish time exceeds 98% of the
deadline. These rules reduce predicted losses; they cannot prevent every
unexpected late result or cost increase.

Without a suitable competing bid, the proposed price is based on the larger of
a cost markup and 8% of the budget, while still applying the floor and budget
cap. Price-only award rules are supported too. An unknown award rule is refused
rather than guessed.

The 2%, 20%, and 8% settings are currently fixed policy choices. They have not
yet been learned from a complete history of wins and losses.

## Bids can change during an auction

When the best visible competing score changes, the client can revise a pending
proposal. Repeated copies of the same public state and our own bid echoes do
not cause repeated identical proposals.

If no profitable replacement is available, the client still tracks the bid
already sent. The protocol does not promise that sending a refusal cancels an
existing proposal, so forgetting that bid could leave an awarded task unhandled.

Prices use fixed increments of $0.0001. The same task, calibration, observations,
queue, and public snapshot produce the same decision. A different machine load
or a new market snapshot can change the inputs and therefore change the bid.

## What learning does not do yet

It does not save a complete history of every auction and rejection. It does not
learn the best undercutting margin. It does not cache completed task answers:
a repeated task is still calculated again, even when its timing is familiar.
Those are [possible next improvements](05-next-improvements.md).

The implementation is in [the strategy](../../src/strategy.rs),
[timing learning](../../src/bidder.rs), and [market feed](../../src/market.rs).
