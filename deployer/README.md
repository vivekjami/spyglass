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
| `propose <service> <to_version> --eid E1 [--eid E2 …] [--ttl-secs 600]` | record a rollback **proposal**: mints the `proposal_id`, snapshots the current version as `expected_current`, stamps an expiry; no routing change | **yes** (`propose_rollback`, not gated) |
| `execute <proposal_id>` | execute a recorded proposal (restated from the proposal itself) — the operator's path through the same checks the gate applies | **yes** as `rollback(proposal_id, …)`, **gated** (Phase 9) |
| `rollback <service> <to_version> --request-id <uuid> [--expected-current <v>] [--eid E1 --eid E2 …]` | the operator's manual rollback with a caller-supplied key; the library function the gated path ends in | no (Phase 3's agent path, retired in Phase 9) |
| `current [service]` | print routing state | read-only |
| `journal` | print the journal | read-only |

Every command prints the journal entry it produced as one JSON line.

## Rollback semantics (Phase 9)

The agent never supplies the idempotency key. It **proposes**, then the gated
`rollback` **consumes** the proposal:

1. **`propose`** validates the target (known service/version; not already at
   `to_version`; at least one `E<n>` evidence id), mints a v4 `proposal_id`,
   snapshots the current version as `expected_current`, stamps `expires_at`
   (default 600 s — the harness's approval gate never expires on its own, so
   this is the only clock), and journals a `proposal` entry. No `D-n` is
   consumed.
2. **`rollback(proposal_id, service, to_version, expected_current, justification_eids)`**
   — the restatement is what the approver reads at the gate; the deployer
   requires it to equal the proposal (service, version, expected_current if
   given, evidence ids as a set) → otherwise `aborted: restated proposal
   differs…`.
3. **Idempotent on `proposal_id`.** An executed proposal re-sent is journaled
   as `noop` (*duplicate proposal_id; already executed as entry n=…*). A
   retrying agent that double-fires is harmless by construction.
4. **Expiry.** Past `expires_at` → `aborted: proposal expired…`; the world does
   not move; re-propose against the current state.
5. **TOCTOU check.** If the live version is no longer `expected_current` — an
   operator changed it while the approval was pending — `aborted: version
   mismatch…`, exit **2** on the CLI. The agent must re-propose against reality.
6. **Already there** → `noop`. Otherwise: new `D-n`, `from_version` recorded,
   `justification_eids` recorded (validated for shape here; their meaning is
   the engine ledger's).

Every path — proposal, rollback, noop, aborted with its reason — is a journal
entry. `just s9-check` exercises all of them live.

## Journal entry

```json
{"n":5,"kind":"rollback","deploy_id":"D-3","service":"payments","version":"v1",
 "from_version":"v2","ts":"2026-08-28T12:27:00.119Z","actor":"agent",
 "request_id":"e7a006fc-…","justification_eids":["E1","E2"]}
```

`kind` ∈ `init | deploy | proposal | rollback | noop | aborted`. Proposals carry
`expected_current` and `expires_at`. `deploy_id` is present only
on entries that changed routing, and counts only those — so from a reset
journal the ids are deterministic (`D-1`, `D-2`, …) and ground truth can name a
culprit change before any run.

## Known versions

Hard-coded in `KNOWN_VERSIONS`: `gateway` v1 · `orders` v1, v1.1 · `payments`
v1, v2. A deploy to anything else is refused. Config file: later, if a scenario
needs it.
