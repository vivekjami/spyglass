# ADR-011 — One action, one gate, a human on it

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 9, when the action path was hardened)

## Context

A rollback in production terms is irreversible enough: it changes what
customers hit, and a wrong one turns an incident into two. The hackathon
judges control and safety explicitly; the author believes it regardless.
Phase 3 shipped the crude version — a `rollback` tool behind the harness's
approval gate, with the agent supplying its own idempotency key and its
own "expected current version". Phase 2 showed why that is not enough: the
model's "fresh UUID" was `9b8c2d1e-3f4a-5b6c-7d8e-9f0a1b2c3d4e`, ascending
hex nibbles, a pattern rather than entropy — an idempotency key that a
*later* incident's rollback could collide with and silently not happen.

## Decision

1. **Exactly one mutating tool exists**, `rollback`, on its own MCP server
   (the write plane never shares a process with the evidence plane), marked
   `require_approval_for_tools` in every agent manifest. `deploy` is not
   exposed. Adding any future mutation requires the same pattern — own tool,
   own gate, own idempotency key, own verification — and this document is
   what scope creep has to argue with.
2. **The system mints the idempotency key; the model never does.** The
   agent calls `propose_rollback(service, to_version, justification_eids)`
   — non-mutating, journaled — which mints a v4 `proposal_id`, snapshots
   the live version as `expected_current`, and stamps an expiry. The gated
   `rollback(proposal_id, …)` *consumes* the proposal.
3. **The human approves what they can read, and that is what runs.** The
   `rollback` call restates service, version, `expected_current` and the
   evidence ids, so the gate renders them; the deployer refuses — and
   journals `aborted` — if the restatement differs from the minted
   proposal. The runner resolves each cited evidence id to the ledger line
   that produced it and prints that at the gate, so the approver sees
   *"E8: replay v2 20/20 failed"*, not a bare id.
4. **Stale approvals never execute.** Proposals expire (600 s, config).
   The harness's gate has no timeout of its own — a pending approval sits
   in a map until answered (Phase 9 finding) — so the deployer's clock is
   the only one, and it is enforced where the action happens.
5. **TOCTOU is checked at execution, not at proposal.** If the live version
   is no longer `expected_current` — an operator fixed it by hand while the
   gate was pending — the rollback is `aborted: version mismatch`, nothing
   changes, no deploy id is minted, and the agent must re-propose against
   reality (once; the SOP forbids a third attempt).
6. **A repeat is a no-op.** The same `proposal_id` sent twice — a retrying
   agent, a double-clicked approval — is journaled `noop: duplicate
   proposal_id` and does not roll back a second time.
7. **Success is never assumed.** After an executed action the engine, not
   the agent, judges recovery (`verify_recovery`, C11): two consecutive
   clean checks close the incident with a `verified_recovery` ledger entry;
   a rate no better than the incident, rising across dirty checks, or still
   dirty five minutes after the action writes an `escalation` entry and the
   SOP stops — no second action of any kind. Denied approvals are terminal
   for the same reason.

Every path — proposal, execution, no-op, every refusal with its reason —
is a journal entry; the benchmark scores verification from the ledger's
closing entry, not from the prose.

## Alternatives considered

- **Full autonomy behind confidence thresholds.** Rejected: thresholds are
  exactly what miscalibrated models fake, and a causal replay (ADR-010)
  is stronger evidence than any self-reported confidence — yet it still
  goes through the gate.
- **Allow-lists of "safe" mutations.** Rejected for v0: one gate, one
  action, zero ambiguity.
- **The runner injects a real UUID into the model's call.** Rejected: it
  fixes the entropy but not the accountability — the key would belong to
  the harness plumbing, not to a recorded proposal a human approved.
- **Gate the proposal too.** Rejected: a proposal changes nothing; gating
  it would double the human's clicks for no added control.

## Consequences

- MTTR includes human latency — measured and shown, because that *is* the
  honest number.
- Two extra tool calls per action (`current_versions`, `propose_rollback`)
  and the verification checks; each is a ledger entry.
- An agent that reads its own proposal back wrong is refused, not
  corrected: the deployer never "helpfully" substitutes the minted values.
- The engine's per-investigation call budget (`[limits]`) is the floor
  under the harness's `iteration_limit`; the 61st call in a minute is
  refused with an instruction to synthesise from what the agent has.

## Reversal conditions

Graduated autonomy — letting a `separated` replay plus a clean
`verify_recovery` history approve a *second* identical rollback without a
human — is Future / optional and out of hackathon scope. If it is ever
built, it lands as a policy on the gate, not as a bypass of it.
