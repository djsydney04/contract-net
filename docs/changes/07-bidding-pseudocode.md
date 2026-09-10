# Bidding agent pseudocode

[Back to the implementation guide](../README.md)

This describes the default Rust agent, including the bidder optimizations
introduced in `f3223b0` and the subsequent safeguards for overlapping auctions.
It follows [strategy.rs](../../src/strategy.rs),
[bidder.rs](../../src/bidder.rs), [market.rs](../../src/market.rs), and
[client.rs](../../src/client.rs). Networking error handling is summarized; pricing
constants, learning rules, queue accounting, and revision conditions are shown
explicitly.

Durations below are in seconds. Manager timestamps and the market close-window
check use milliseconds. `NONE` means no valid estimate, competitor, or bid,
depending on the function. An auction request is called a CFP, meaning a call
for proposals.

`task.type`, `task.ID`, and `task.deadline` abbreviate the source fields
`task_type`, `task_id`, and `deadline_s`.

## How the original bidding rule worked

The original Python agent, and later the Rust agent before bidder optimization,
used a markup on local computation:

```text
compute = estimated_work(task) / calibrated_rate(task.type)
finish = queue_seconds + compute
price = compute * rules.cost_rate * 1.6

if finish > task.deadline or price > task.budget:
    refuse
else:
    propose(price, estimated_seconds = finish)
```

Queue time affected the completion estimate, but the price covered only local
compute time. Delivery overhead and public competing bids did not affect the
price. The archived Rust version also validates numeric inputs; its full source
is preserved in [the baseline fixture](../../tests/fixtures/bidder-before-optimization.rs).

## State and event flow

```text
state:
    rules                   # current manager rules
    rates                   # calibrated work units per second, by task type
    observations            # latest 512 valid timing/settlement records
    commitments             # task ID -> pending, awarded, running, or complete
    award_queue             # awarded task IDs, in arrival order
    active_worker           # handle, start time, reserve; at most one calculation
    market_snapshot         # latest public state plus local receipt time
    result_outbox           # results waiting to be sent

startup:
    load and validate saved observations if state storage is enabled
    measure rates: two warmups, then median of five timed runs per type
    start the optional read-only spectator connection
    connect and register with the manager

on REGISTERED:
    update rules
    evaluate the manager's open CFPs

on CFP(task):
    evaluate a proposal using the steps below

on public market update:
    store the snapshot and its local receipt time
    if registered, reconsider pending proposals whose competing score changed

on ACCEPT_PROPOSAL(task_id):
    if the tracked commitment is pending:
        mark awarded; record local award-receipt time
        append to award_queue
        start the next task if the worker is idle

on REJECT_PROPOSAL or BID_INVALID:
    remove the corresponding commitment only if it is still pending

on worker completion:
    for a matching RUNNING task and attempt:
        mark complete and record local runtime on success
        enqueue INFORM with the exact integer answer, or FAILURE on error
    start the next queued task
    send queued results when registered and connected

on SETTLED:
    update learning from eligible timings
    remove the task's tracked commitment, queue entry, and unsent result
    retain any active worker until its calculation actually finishes
    append the settlement to the session history
```

The worker runs on a separate thread while the asynchronous loop handles
messages. There is no completed-answer cache: repeated inputs still execute.
The default rejection hooks do not train a win-probability model.

## 1. Establish the baseline and queue estimate

```text
function BASELINE_SECONDS(task):
    rate = rates.get(task.type)
    if rate is missing, nonfinite, or <= 0:
        return INFINITY
    work = estimated_work(task)
    if work cannot be calculated:
        return INFINITY
    return work / rate

function QUEUE_SECONDS():
    total = 0
    for commitment in deterministic task-ID order:
        if commitment.stage is PENDING or AWARDED:
            total += commitment.compute_reserve
    if active_worker exists and its handle is not finished:
        remaining = active_worker.compute_reserve - time_since_worker_start
        if remaining <= 0:
            return INFINITY
        total += remaining
    return total
```

Queue accounting includes pending proposals as well as awarded and running
work. This reserves capacity for work that might be awarded. Remove the pending
entry being replaced before calculating its new quote, so it does not count
itself in the queue. A non-pending commitment is not replaced by a new CFP.
The worker is tracked independently of settlement cleanup: a timed-out contract
may still occupy the CPU. Once a worker outlasts its reserve, admission stops
until it finishes. Already awarded work stays queued.

The work estimates match the native algorithms described in
[the function history](06-function-optimizations.md).

## 2. Forecast compute time and delivery overhead

`PERCENTILE` sorts the sample and picks the element at one-based position
`ceil(percent * sample_count / 100)`. An empty sample uses the stated fallback.

```text
function FORECAST(task, baseline):
    key = SHA256(task.type + ":" + serialized_JSON(task.params))
    matching = observations with task_key == key

    if matching is empty and task.type != "hash_search":
        matching = observations with the same task type

    ratios = [o.local_seconds / o.baseline_seconds for o in matching]
    cold_reserve_factor = 3.0 for hash_search, otherwise 1.2
    reserve_factor = PERCENTILE(ratios, 95, fallback = cold_reserve_factor)
    expected_factor = PERCENTILE(ratios, 50, fallback = 1.0)
    sample_margin = 1.1 if matching.count < 5, otherwise 1.05

    compute_reserve = max(baseline * reserve_factor * sample_margin, 0.000001)
    expected_compute = max(baseline * expected_factor * 1.05, 0.000001)

    overhead = []
    for o in all observations:
        if o.cost_rate > 0:
            billed_seconds = o.cost / o.cost_rate
        else:
            billed_seconds = o.manager_seconds
        residual = max(o.manager_seconds, billed_seconds)
                   - o.local_seconds - o.queue_seconds
        append max(residual, 0) to overhead

    delivery = max(PERCENTILE(overhead, 95, fallback = 0.050), 0.005) + 0.005
    return expected_compute, compute_reserve, delivery
```

Without history, delivery is 55 ms. The minimum learned allowance is 10 ms.
An unseen hash input uses a larger compute reserve because its required number
of attempts can vary substantially; another seed's timing is not used as a
direct substitute. The cold factors are heuristics, not measured guarantees.

Task IDs and auction attempts are not part of the timing key. The task type and
input values identify the calculation. Delivery residuals are pooled across all
task types and can include unseparated processing or network delay.

## 3. Select a valid competing score

```text
function SCORE(rules, price, seconds):
    if rules.award_policy == "best_value":
        if time_weight is nonfinite or < 0:
            return NONE
        value = price + rules.time_weight * seconds
    else if rules.award_policy is "lowest_price" or "cheapest":
        value = price
    else:
        return NONE
    return value if finite, otherwise NONE

function BEST_COMPETING_SCORE(task):
    if no snapshot or local snapshot age >= 2 seconds:
        return NONE
    if snapshot award policy or time weight differs from our rules:
        return NONE

    find an active auction matching:
        task ID, type, full inputs, attempt, budget, and deadline
        state == "bidding"
        bids_close_at > snapshot.now + 200 milliseconds
    if no such auction:
        return NONE

    candidates = live bids for that task ID where:
        agent != our name
        outcome is absent or null
        price is finite and 0 <= price <= task.budget
        estimated_seconds is finite and 0 <= estimated_seconds <= task.deadline

    return minimum valid SCORE among candidates, or NONE
```

The close-window check uses the manager time inside the snapshot. The separate
two-second expiry uses local receipt time. This describes the current freshness
checks; it does not assume knowledge of future bid changes.

## 4. Choose a price or refuse

```text
function CHOOSE_BID(task, queue, forecast, competing_score):
    cost_rate = rules.cost_rate
    budget = task.budget
    deadline = task.deadline
    quoted_time = queue + forecast.expected_compute + forecast.delivery
    reserved_time = queue + forecast.compute_reserve + forecast.delivery

    if quoted_time is nonfinite or <= 0:
        return NONE
    if cost_rate is nonfinite or < 0:
        return NONE
    if budget, deadline, or reserved_time is nonfinite:
        return NONE
    if reserved_time > task.deadline * 0.98:
        return NONE

    cost = reserved_time * rules.cost_rate
    minimum_price = max(cost * 1.2 + 0.01 * max(rules.cost_rate, 0.1), 0.0001)
    time_component = SCORE(rules, price = 0, seconds = quoted_time)
    if time_component is NONE:
        return NONE

    if competing_score exists:
        ceiling = competing_score * 0.98 - time_component
    else:
        ceiling = max(cost * 1.6, task.budget * 0.08)

    ceiling = min(ceiling, task.budget * 0.98)
    price = floor(ceiling * 10000) / 10000
    if price is nonfinite or price < minimum_price:
        return NONE
    return Bid(price, estimated_seconds = quoted_time)
```

This calculates a score ceiling against the competitor, not simply a price 2%
below the competitor's price. The time component matters under `best_value`.
The cost floor remains mandatory even if meeting it means losing or refusing
the auction. The 2%, 20%, 8%, and 60% settings are fixed policy parameters.

## 5. Track and submit the proposal

```text
function HANDLE_CFP(task):
    if a commitment exists for task.ID and is not PENDING:
        return without sending another proposal
    previous = temporarily remove any older pending entry for task.ID

    baseline = BASELINE_SECONDS(task)
    forecast = FORECAST(task, baseline)
    queue = max(QUEUE_SECONDS(),
                max(rules.concurrency - 1, 0) * forecast.compute_reserve)
    competitor = BEST_COMPETING_SCORE(task)
    bid = CHOOSE_BID(task, queue, forecast, competitor)

    total_compute = QUEUE_SECONDS() + forecast.compute_reserve
    capacity_fits = total_compute is finite and, for every PENDING commitment:
        total_compute <= commitment.queue_allowance + commitment.compute_reserve

    if bid is NONE, fails the client checks below, or capacity_fits is false:
        if previous exists:
            restore previous without sending REFUSE
        else:
            send REFUSE(task.ID)
        return

    store a PENDING commitment containing:
        task, bid, competitor score, baseline, and compute reserve
        queue_allowance = queue
        unset award/start times and local runtime
    send PROPOSE(task.ID, bid.price, bid.estimated_seconds)
```

The client checks that price is finite and between zero and budget, and that
estimated time is finite and between zero and deadline. It also catches strategy
panics so a broken custom hook does not take down the connection loop.
Queue time and compute reserve must be finite; compute reserve must be
nonnegative. The default strategy always supplies its learned compute reserve;
custom strategies with a nonfinite reserve can fall back to the finite quote
minus its queue allowance.

The manager's concurrency setting determines the initial waiting allowance,
while execution remains serial. Admission protects every existing pending bid
against being awarded after the new job. The check uses each task's own compute
reserve and does not assume equal runtimes. Price and deadline calculations
include the same saved queue allowance. This can refuse a large new job even
when that job's own deadline is generous, because it could delay a smaller
pending task beyond its quote.

The pseudocode shares one forecast for readability. The current implementation
calculates it through both `compute_reserve` and `on_cfp`; removing that repeated
calculation is still a [proposed improvement](05-next-improvements.md).

## 6. Reprice pending proposals on market updates

```text
for each PENDING commitment in task-ID order:
    previous = temporarily remove the commitment
    competitor = BEST_COMPETING_SCORE(previous.task)

    if competitor is NONE or equals previous.competitor_score:
        restore previous
        continue

    calculate queue allowance, baseline, forecast, and bid as for a new CFP
    check capacity against the other pending commitments
    if no valid replacement bid or insufficient queue capacity:
        restore previous
        continue without sending REFUSE

    store the replacement PENDING commitment and its current competitor score
    if new price != previous price
       or abs(new estimated_seconds - previous estimated_seconds) >= 0.0001:
        send the replacement PROPOSE
```

Repricing preserves an already submitted bid when a profitable replacement is
unavailable because `REFUSE` is not documented to cancel it. Failed ordinary CFP
replacements preserve the previous proposal for the same reason.
Unchanged competitor scores are skipped, avoiding
repeated proposals in response to our own bid echoes.

## 7. Learn from settlement

```text
on settlement with a tracked commitment:
    if verdict is "correct" or "late"
       and local runtime, manager runtime, start time, and award time exist:
        candidate = Observation(
            task_key = hash of task type and inputs,
            task_type,
            baseline_seconds = stored baseline,
            local_seconds = measured execution time,
            queue_seconds = max(start_time - award_receipt_time, 0),
            manager_seconds = settlement.runtime,
            cost_rate = current rules.cost_rate,
            cost = settlement.cost,
            profit = settlement.profit
        )
        if candidate passes validation:
            append candidate
            while observations.count > 512:
                discard the oldest observation
        mark learning state for saving

    after normal settlement cleanup, if saving is enabled and state is marked:
        write a versioned snapshot to a temporary file on a worker thread
        rename it over the normal state file
        await completion before the connection loop's next iteration
```

Validation requires finite, nonnegative timing, cost, and cost-rate values;
baseline and local execution times must be positive, and manager runtime must
be at least local execution time. Profit must be finite but may be negative.
Invalid observations are discarded. File loading applies the same validation
and history bound.

Only outcomes already received influence subsequent forecasts. Complete auction
history, trained winning probabilities, background saves that never suspend the
connection loop, and result caching are not implemented by this pseudocode.

## What is repeatable, and what is measured

For identical decoded inputs, calibration, queue, observations, and public
snapshot, the default pricing calculation is deterministic. Real arrival times,
machine load, and competing proposals can change those inputs. The policy has
no random exploration step.

The [backtest report](../../graph/bidder/README.md) compares this policy with the
archived pre-optimization Rust agent using controlled inputs and explicit
delivery assumptions. Its profit curves are simulations. This document does
not add a new benchmark run or imply that the pseudocode guarantees live wins.
The saved replay is sequential and does not evaluate profit under overlapping
awards. The [queue tests](../../tests/integration/queue.rs) cover simultaneous
auctions, differing task sizes, reversed award order, duplicate awards,
rejected/replaced bids, deadline admission, worker overruns, and early settlement.
