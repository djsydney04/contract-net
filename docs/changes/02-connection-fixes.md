# The connection and message fixes

[Back to the guide](../README.md)

## Why valid messages were being ignored

The repeated “ignoring malformed manager message” logs came from a decoding
problem in the Rust client. The manager can send ordinary decimal values, such
as a penalty rate of `0.5`, alongside task inputs containing very large whole
numbers.

The original decoding approach tried to sort a message into its category while
also buffering its fields. That combination did not handle decimal fields
correctly with the option needed to preserve large integers. A valid
registration response could therefore be rejected, leaving the client waiting
until registration timed out.

The client now reads the JSON message first, examines its `type`, and then
decodes the matching message body. JSON is the text format used to send named
values between the client and manager. This approach preserves both decimals
and exact large integers. The public auction feed uses the same principle.

Tests cover decimal rules and large task parameters. Unknown message types can
be skipped, while incorrectly formatted known messages still produce useful
errors. This fix addresses the decoding failure seen in the logs; a different
network or credential problem can still prevent registration.

## Calculation and networking have different jobs

The client keeps its connection open while a separate worker calculates task
answers. It can continue receiving messages instead of waiting for a long
calculation to finish before listening again.

Awarded tasks currently execute one at a time. A queue holds additional work,
and its estimated waiting time affects later bids. Receiving the same award
again does not launch the calculation a second time.

The client registers on every new connection, applies the manager's current
rules, handles auctions already open at registration, and exchanges keepalive
messages so it can detect a lost connection. Small outgoing messages are sent
without the optional TCP delay that could otherwise hold them briefly.

## What happens when the connection drops

A completed result that has not been sent successfully is kept in memory and
can be sent after reconnecting and registering again. Old pending bids are
discarded during recovery. This is useful recovery within the same process;
it is not a durable record of every outstanding contract.

Saved timing observations can survive a restart. Unfinished work, queued
contracts, and unsent results do not survive the process exiting. Being evicted
for a duplicate team name stops the client rather than repeatedly reconnecting
and displacing the other instance. Invalid credentials also stop it with an
error.

Ctrl-C stops the application promptly. It does not wait indefinitely for a CPU
calculation that is already running.

## The second connection only watches public auctions

The bidder can open a separate spectator connection to observe public bids.
That connection does not register a contractor or submit proposals. The main
connection remains responsible for our actual participation.

The spectator feed reconnects independently. If suitable public data is
unavailable, bidding falls back to the cost-and-budget rule described in
[the bidder guide](03-bidding.md). `--no-market-feed` disables the spectator
connection.

The client ignores its own bids and public bids that are invalid, already
settled, too old, or attached to a different auction. Matching includes the
task inputs and attempt, not only the task number, because practice task
numbers are reused.

## A remaining limitation

Saving learned state happens on a separate worker, but the connection loop
currently waits for that save to finish before its next iteration. Moving
saves fully into the background is a proposed improvement, described in
[the next-steps page](05-next-improvements.md). It has not been implemented.

See [the protocol reference](../PROTOCOL.md) for the exact message fields and
[the client source](../../src/client.rs) for the current behavior.
