# deployer

The **write plane**. Versioned deploy/rollback for the target system with an
append-only journal. The evidence engine never touches this; the agent reaches
it only through the `rollback` MCP tool (Phase 3), behind the harness approval
gate. Design: [ADR-017](../docs/adr/ADR-017-routing-by-file-versions-always-on.md).

```
cargo build --release -p deployer
./target/release/deployer --data-dir data/deploy <command>
```

## Files it owns (`--data-dir`, default `data/deploy`)

| File | What | Who reads it |
|---|---|---|
| `current.json` | version + deploy id each service is routed to; written with write-then-rename | `orders` (per request, via a read-only bind mount) |
| `journal.jsonl` | every deploy / rollback / no-op / abort, append-only; its own WAL | the engine (`deploy_events`), the benchmark, humans |

## Commands

| Command | Effect | Exposed to the agent? |
|---|---|---|
| `init [--reset]` | every service at `v1`; `--reset` rotates the journal and starts clean | no |
| `deploy <service> <version> [--actor A]` | route `service` to `version`; journal `deploy` with a new `D-n` | **no** — scenario setup only |
| `rollback <service> <to_version> --request-id <uuid> [--expected-current <v>] [--eid E1 --eid E2 …]` | the one mutating action; see semantics below | **yes**, gated (Phase 3) |
| `current [service]` | print routing state | read-only |
| `journal` | print the journal | read-only |

Every command prints the journal entry it produced as one JSON line.

## Rollback semantics

1. **Idempotent on `request_id`.** A `request_id` already acted on is journaled
   as `noop` (*duplicate request_id; original entry n=…*) and nothing changes.
   A retrying agent that double-fires is harmless by construction.
2. **TOCTOU check.** `--expected-current` names the version the proposal was
   made against. If the actual current version differs — the world moved
   between approval and execution — the call is journaled as `aborted` and
   exits **2**. The agent must re-propose against reality.
3. **Already there** → `noop` (*already at requested version*).
4. Otherwise: new `D-n`, `from_version` recorded, `justification_eids` recorded
   (not validated here — the ledger does that).

## Journal entry

```json
{"n":5,"kind":"rollback","deploy_id":"D-3","service":"payments","version":"v1",
 "from_version":"v2","ts":"2026-08-28T12:27:00.119Z","actor":"agent",
 "request_id":"e7a006fc-…","justification_eids":["E1","E2"]}
```

`kind` ∈ `init | deploy | rollback | noop | aborted`. `deploy_id` is present only
on entries that changed routing, and counts only those — so from a reset
journal the ids are deterministic (`D-1`, `D-2`, …) and ground truth can name a
culprit change before any run.

## Known versions

Hard-coded in `KNOWN_VERSIONS`: `gateway` v1 · `orders` v1, v1.1 · `payments`
v1, v2. A deploy to anything else is refused. Config file: later, if a scenario
needs it.
