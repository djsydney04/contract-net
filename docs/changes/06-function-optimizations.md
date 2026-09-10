# Function optimizations from the original implementation

[Back to the implementation guide](../README.md)

The optimization history has three stages: the original Python reference,
optimized Python task execution, and the native Rust application. Later bidder
changes improved runtime estimates and pricing without changing task answers.
This page follows those stages for each function and identifies the source of
each change.

Array indices in the pseudocode start at zero.

## Source revisions

| Stage | Source | What changed |
|---|---|---|
| Original, `4e475fb` | [Reference tasks](https://github.com/djsydney04/contract-net/blob/4e475fb7be0e20b8d3753fd1624471eaebf8c7f6/contractnet/tasks.py) | Defined the required integer answers and seeded random sequences. The contractor initially delegated execution to these functions. |
| Python optimization, `aecaea2` | [Custom executor](https://github.com/djsydney04/contract-net/blob/aecaea260662975789fcccfe90b5e42fbfbd2d8f/my_contractor.py) | Overrode task execution with cheaper algorithms and reduced Python overhead. The reference task module stayed unchanged. |
| Rust migration, `7607ef1` | [Native tasks](https://github.com/djsydney04/contract-net/blob/7607ef1de238d67741f81e66dd0f29b7d31c341e/src/tasks.rs) and [random generator](https://github.com/djsydney04/contract-net/blob/7607ef1de238d67741f81e66dd0f29b7d31c341e/src/random.rs) | Preserved the optimized algorithms and exact outputs, implemented them in Rust, and removed the Python runtime dependency. |
| Bidder optimization, `f3223b0` | [Timing model](https://github.com/djsydney04/contract-net/blob/f3223b0c9ba2e0a86edb7f7e4e6ccdc16fc47862/src/bidder.rs) and [calibration](https://github.com/djsydney04/contract-net/blob/f3223b0c9ba2e0a86edb7f7e4e6ccdc16fc47862/src/benchmark.rs) | Added steadier calibration, learned compute and delivery estimates, and pricing based on elapsed cost and visible competition. |

The current task implementations are in [src/tasks.rs](../../src/tasks.rs).
The major prime and matrix algorithm changes first appeared in the optimized
Python executor; they were carried into Rust rather than introduced by the
language change itself.

## `monte_carlo_pi`: preserve the algorithm, reduce execution overhead

**Original:** create a seeded generator, draw two floating-point values per
sample, and count points satisfying `x*x + y*y <= 1`. Despite the task name,
the answer is the integer count, not an approximation of pi sent as a float.

**Optimized Python:** bind `rng.random` to a local variable before the loop.
This avoids repeating the method lookup on every draw. The number of samples,
draw order, and circle test stay the same.

**Rust:** run the loop in native code using the reference-compatible generator.
The two draws still occur in the same order and the boundary test remains
`<= 1.0`. The improvement comes from implementation overhead rather than fewer
samples. Work remains O(samples), with constant-size generator and counter state.

```text
random = reference_compatible_generator(seed)
inside = 0
repeat samples times:
    x = random.next_float()
    y = random.next_float()
    if x*x + y*y <= 1:
        inside += 1
return inside
```

Skipping samples or using a different random sequence would change the answer,
even if the resulting pi estimate were statistically reasonable.

## `prime_count`: replace repeated division with a segmented sieve

**Original:** inspect every candidate in `[max(lo, 2), hi)`. Count 2 directly,
skip other even values, and try odd divisors up to the candidate's square root.
A long interval repeats many division operations against the same divisors.

**Optimized Python:** first build the primes up to `floor(sqrt(hi - 1))`. Then
process the requested interval in sections of at most 1,000,000 values, marking
multiples of those primes as composite. Count the values left unmarked.
Byte arrays and slice assignments perform the marking efficiently.

**Rust:** retain that segmented sieve using native loops and boolean candidate
storage. Use an integer square root and wider arithmetic to calculate the first
multiple in each section without overflowing the normal bound type.

```text
lo = max(lo, 2)
if hi <= lo:
    return 0

base_primes = sieve_primes_through(integer_sqrt(hi - 1))
total = 0
for each [start, stop) section of [lo, hi), with at most 1,000,000 values:
    candidates = all true
    for p in base_primes:
        first = max(p*p, ceil(start / p) * p)
        mark first, first+p, ... below stop as composite
    total += count of true candidates
return total
```

Starting at `p*p` avoids marking the prime itself. `hi` remains excluded, as in
the reference. Segmentation bounds the interval buffer, but the base sieve
still grows with `sqrt(hi)`; this is not constant-memory prime counting.
The main gain is replacing repeated trial division with shared marking work.

## `hash_search`: reuse the fixed hash input and remove per-attempt allocation

**Original:** for each candidate number, format the full string `seed:nonce`,
encode it, calculate SHA-256, and interpret its first four bytes as a big-endian
integer. Return the first candidate below the threshold.

**Optimized Python:** initialize the hash with `seed:` once, copy that state
for each attempt, and append only the decimal candidate. Compare the first
four digest bytes with the four-byte threshold. Equal-length big-endian byte
comparison preserves the integer comparison while avoiding that conversion.

**Rust:** keep the reusable prefix state. Format the candidate into a fixed
20-byte buffer instead of allocating a new string for each attempt. Read the
four digest bytes directly into a native `u32` and compare with the threshold.

```text
if threshold <= 0:
    fail with invalid threshold
if threshold >= 2^32:
    return 0

prefix = SHA256_state_after_bytes(seed + ":")
for nonce from 0 through the supported integer range:
    candidate = clone(prefix)
    candidate.append(decimal_bytes(nonce))
    digest = candidate.finish()
    if big_endian_u32(digest[0..4]) < threshold:
        return nonce
fail if the supported range is exhausted
```

The Python optimization also added the threshold guards, avoiding an impossible
search for nonpositive targets. The Rust search is bounded by `u64` and reports
an error if exhausted.

The search order and first accepted candidate are unchanged. There is no claim
that the optimized code needs fewer guesses. Under the usual uniform-hash model,
expected attempts are `2^32 / threshold`, but a particular input can take much
longer or finish immediately. Prefix reuse and allocation removal lower the
cost per attempt.

## `sort_checksum`: generate the same values with less overhead

**Original:** generate `n` values using `randrange(0, 2^31)`, sort them, and add
each value multiplied by its one-based position, reducing modulo `2^61 - 1`.

**Optimized Python:** bind `getrandbits` locally and reproduce the exact rejection
sampling used by `randrange`. For this bound, it draws 32 bits and rejects values
at or above `2^31`. It also calculates the weighted sum before one final modular
reduction; Python integers can grow to hold that sum.

Drawing only 31 bits would produce values in range but consume the random stream
differently. Matching the reference requires preserving the rejected draws too.

**Rust:** store values in a `u32` array, generate them with the same rejection
rule, and use `sort_unstable`. Sort stability is irrelevant here because equal
values are interchangeable in the numeric checksum. Accumulate in `u128` and
reduce after each term to keep the accumulator bounded.

```text
values = empty array
repeat n times:
    repeat:
        value = reference_random_32_bits()
    until value < 2^31
    append value

sort values in ascending order
total = 0
for index, value in values:
    total = (total + (index + 1) * value) mod (2^61 - 1)
return total
```

Sorting still takes O(n log n) work and the array uses O(n) storage. The changes
reduce generation, allocation, and execution overhead rather than eliminating
the sort. Moving modular reduction through an integer sum preserves the final
remainder.

## `matmul_mod`: calculate the checksum without a full matrix product

**Original:** generate two `n × n` matrices, transpose the second, and calculate
every output cell with a dot product. The transpose already improves how the
inner loop reads data, but the algorithm still performs O(n³) arithmetic and
stores O(n²) values. The task returns only the sum of the product's entries,
reduced modulo `mod`.

**Optimized Python:** use the fact that only the total is needed. For output
`C = A × B`, expand the sum and group the terms by the shared index `k`:

```text
sum of C[i,j]
    = sum over i,j,k of A[i,k] * B[k,j]
    = sum over k of (sum over i of A[i,k]) * (sum over j of B[k,j])
    = sum over k of column_sum_A[k] * row_sum_B[k]
```

Generate all of A first, accumulating its column totals. Then generate B in the
original row order, calculate each row total, and combine it with the matching
column total. This preserves the reference's random draw order while reducing
the work to O(n²) and stored totals to O(n).

**Rust:** preserve the same identity. For moduli that fit `u64`, use `u128`
intermediates and reduce column totals, row totals, and accumulated products
modulo `mod` to prevent overflow. Larger moduli use arbitrary-precision integers.
Both paths return the same final remainder.

The following shows the native-sized path:

```text
columns = n zeros
random = reference_compatible_generator(seed)

for each row of A:
    for k from 0 to n-1:
        columns[k] = (columns[k] + random.below(mod)) mod mod

total = 0
for k from 0 to n-1:                 # generate row k of B
    row_total = 0
    repeat n times:
        row_total = (row_total + random.below(mod)) mod mod
    total = (total + columns[k] * row_total) mod mod
return total
```

This shortcut is valid for the requested total checksum. It would not provide
the full product matrix if a different task required every output cell.
The complexity figures count scalar operations; very large moduli also increase
the cost of each integer operation.

## Supporting functions: preserve compatibility and estimate the right work

The Rust [random generator](../../src/random.rs) reproduces integer seeding,
53-bit floating-point draws, and rejection sampling from the reference. Large
integer seeds are retained rather than truncated to one machine word. A faster
generator with a different sequence would fail the answer checks.

The [calibration module](../../src/benchmark.rs) uses work estimates that reflect
the native algorithms:

| Task | Current work estimate |
|---|---|
| Monte Carlo | Number of samples |
| Prime counting | Interval length plus an allowance proportional to `sqrt(hi)` for the base sieve |
| Hash search | Expected attempts, at least one |
| Sorting | `n * log2(max(n, 2))` |
| Matrix checksum | `n²`, multiplied by an approximate factor for larger modulus sizes |

These are proportional runtime models, not exact instruction counts. In
particular, using the old cubic matrix estimate for a quadratic executor would
misprice larger tasks. Hash calibration performs a fixed 150,000 attempts per
measurement so one lucky or unlucky search does not determine the rate.

The later calibration change added two warmups and the median of five timed
runs. The bidder then adjusts those startup estimates using observed results.
Its behavior is described in [the agent pseudocode](07-bidding-pseudocode.md).

## What the measurements establish

The 218 independent reference answers check that these changes preserve output
compatibility on the saved cases. The [task latency report](../../graph/README.md)
records repeated native execution measurements, including p50, p95, and p99.

The report's older timings come from the already optimized executor log. They
are rounded single observations, not a controlled three-stage benchmark of the
original reference, optimized Python, and Rust. We therefore cannot assign an
isolated measured speedup to every change described here. The complexity changes
and reduced operations are supported by the source; the measured timing claims
remain limited to the datasets identified in the report.

Result caching and learned bidding margins remain
[proposed improvements](05-next-improvements.md), not part of this history.
Awarded tasks still execute serially.
