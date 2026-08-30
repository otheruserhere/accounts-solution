# Accounts solution

```
cargo run -- tests/fixtures/sample.csv
```

With logs enabled:

```
RUST_LOG=debug cargo run -- tests/fixtures/sample.csv
```

## Implementation

We currently hold all `txs` data in memory, so this solution would not scale. For a real-world implementation, we would probably need to use an external data source to keep access to processed transaction data per account.

I did not add any concurrency, since we write to global storage, and lock contention would likely kill any benefit of using multiple threads.

It should be possible to scale this system in a "Kafka-like" way. By partitioning data by client IDs, we could stream all the operations targeting a specific client ID to the same processing engine. So, by adding more partitions, we could horizontally scale our system because each event stream is independent. The downside is that each processor could only produce a CSV for the accounts it owns.

Correctness is validated with a suite of unit tests simulating `Ledger` state changes based on different `Operation` streams - the dispute lifecycle, duplicate transaction IDs, overflow, and locked accounts. I've also added `tests/integration.rs`, which runs the compiled binary against the sample CSVs in `tests/fixtures/` and asserts the exact output, so the program is tested end to end at the CLI boundary. The type system helps too: dispute status is a state machine enum (`Undisputed -> Disputed -> ChargedBack`), so invalid transitions like a double chargeback are rejected by construction, and amounts are `rust_decimal` values, so four-decimal arithmetic is exact with no float drift.

## Assumptions

- Transaction IDs are globally unique. If an ID appears twice, only the first occurrence counts - the duplicate is logged and dropped before any money moves.
- Only deposits can be disputed. The dispute/chargeback mechanics (hold funds, then remove them) only make sense for money that entered the account; applying them to a withdrawal would debit the client twice. A dispute referencing a withdrawal is treated like one referencing an unknown tx: logged and ignored as a partner-side error.
- Dispute requires the tx to exist, belong to the referenced client, and not be already disputed or charged back. Resolve/chargeback additionally require an active dispute. Anything else: logged and ignored.
- A chargeback is terminal - the tx can't be disputed again, and the account locks immediately.
- A locked account ignores all further operations, deposits included. 
- Accounts are created on first reference to a client, even if the operation itself then fails (e.g. a withdrawal with no funds still yields a 0-balance row).
- Withdrawals check available funds only - held funds can't be spent. Failed withdrawals are logged, ignored, and never become disputable.
- Balances can go negative: deposit, withdraw, then chargeback the deposit, and the loss lands on the account.
- Malformed rows (unknown type, bad numbers, missing/negative amount) are logged and skipped; one bad row doesn't kill the run.
