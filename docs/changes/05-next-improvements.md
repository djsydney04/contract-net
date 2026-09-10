# Proposed performance improvements

[Back to the guide](../README.md)

The ideas below were discussed after the completed Rust and bidder work.
**They are proposals, not features already implemented.** The earlier pages
describe what the application does today.

## Record complete auction events

Today, the client saves a bounded collection of timing and settlement
observations. It does not keep a complete record of every opportunity, bid
revision, rejection, and competing bid.

A fuller record would let us explain why we lost an auction and replace borrowed
delivery assumptions with measurements from our own client. It should include
when messages arrived, when proposals were sent, waiting time, calculation time,
and the final result. It should leave registration credentials out.

## Cache results for identical task inputs

The model recognizes repeated task inputs when estimating their timing, but it
still calculates their answers again. A bounded answer cache could reuse a
previously calculated answer when the complete inputs and executor version match.
This may be especially useful for repeated hash searches in a looping room.

We would need separate measurements for a task whose answer is already saved
and a task with unseen inputs. Otherwise a benchmark could make repeated work
look fast without showing the cost of unseen inputs.

## Learn pricing margins from auction outcomes

The current undercutting margin and fallback budget percentage are fixed. With
enough records of wins and losses, we could compare candidate prices and choose
the one with the best expected profit, allowing for the chance of winning and
the likely cost of completing the task.

The cost floor and deadline reserve would still matter. A fixed set of candidate
prices and a consistent way to break ties could keep the decision repeatable.
This requires better auction data first; the small existing backtest does not
establish the best settings.

## Keep file writes out of the connection loop

The application already moves calculation and file operations onto worker
threads. However, the connection loop waits for a state save to finish. A
background writer could combine repeated save requests and let message handling
continue while the disk is busy, with an explicit policy for finishing pending
saves during shutdown.

The current proposal path also calculates its timing forecast twice. Computing
it once and sharing it would remove repeated work. Since the measured bidding
function is already around four microseconds, this is a smaller opportunity
than an avoidable delivery or disk delay.

## Evaluate concurrency, delay spikes, and competitor responses

The saved replay treats competing bids as fixed and handles auctions without
overlapping live work. A stronger evaluation would include overlapping awards,
delayed public updates, occasional large delivery delays, and simulated
competitors that change their bids in response.

We should choose settings using earlier records, then evaluate them on later
records that were not used to choose those settings. Observed historical bid
sequences still cannot tell us exactly how a competitor would react to a
different policy; simulations and controlled live comparisons would add evidence.

Each candidate should be compared with the frozen old bidder and the current
optimized bidder using the same workloads and assumptions. The useful measures
are profit per opportunity, win rate, losing contracts, deadline misses, and
both decision and total-delivery p50/p95/p99.

## Suggested order

Start with complete auction recording and background state saving. Then evaluate
answer reuse and learned pricing as separate changes, with new data and
before/after graphs for each. Keeping these changes separate makes it easier to tell
which one caused an improvement or regression.
