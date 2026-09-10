# Protocol decoding and connection recovery

[Back to the guide](../README.md)

## Decimal fields were breaking message decoding

The repeated “ignoring malformed manager message” logs came from a decoding
problem in the Rust client. Manager messages can contain decimal fields, such
as a penalty rate of `0.5`, alongside task inputs containing large integers.

The original decoder used Serde's internally tagged enum representation to
select a message type and buffer its fields. With JSON `arbitrary_precision`
enabled, that buffered representation could not decode decimal fields into
`f64`, Rust's floating-point type. A valid registration response could therefore
be rejected, leaving the client waiting until registration timed out.

The client now parses each message into `serde_json::Value`, reads its `type`,
and decodes the appropriate payload directly from that JSON value. This avoids
the incompatible intermediate representation while preserving decimal fields
and exact large integers. The public auction feed uses the same approach.

Tests cover decimal rules and large task parameters. Unknown message types can
be skipped, while incorrectly formatted known messages still produce useful
errors. This fix addresses the decoding failure seen in the logs; a different
network or credential problem can still prevent registration.

## Worker execution and network responsiveness

The asynchronous connection loop handles incoming messages while task execution
runs through Tokio's `spawn_blocking`, which moves CPU work onto a worker thread.
A long calculation therefore does not occupy the loop that receives auctions
and sends results.

Awarded tasks currently execute one at a time. A queue holds additional work,
and its estimated waiting time affects later bids. Receiving the same award
again does not launch the calculation a second time.

The client registers on every new connection, applies the manager's current
rules, handles auctions already open at registration, and exchanges keepalive
messages so it can detect a lost connection. Both WebSocket connections enable
`TCP_NODELAY` to avoid the optional TCP delay for small outgoing messages.

## Reconnect and shutdown behavior

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

## Public market observations

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
