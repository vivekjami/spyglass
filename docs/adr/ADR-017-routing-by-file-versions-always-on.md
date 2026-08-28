# ADR-017 — A deploy is a file write; every version is always on

**Status:** Accepted · **Date:** 2026-08-28 (Phase 1)

## Context

Three requirements pull in the same direction:

1. The causal check replays a captured failing request against the suspected
   version *and* the known-good version. Both must be reachable at once.
2. Rollback must be fast and its effect observable in telemetry within
   seconds, or the verification loop (C11) becomes a long wait.
3. The deployer must be TOCTOU-checkable and idempotent, which is far easier
   when "current version" is a single small piece of state rather than
   container status.

Traditional deploy semantics — stop the old container, start the new one — fail
all three: only one version exists at a time, a restart takes seconds and
loses in-flight requests, and "current version" is scattered across the
orchestrator.

## Decision

- Every version that exists as an artifact runs as an **always-on Compose
  service** (`payments-v1`, `payments-v2`).
- **Routing is a file.** `data/deploy/current.json` names the version each
  service is routed to. `orders` reads it on every request (cached on mtime)
  and picks the upstream URL accordingly.
- The deployer writes that file with **write-then-rename**, so readers on the
  read-only bind mount never see a torn state. A deploy or rollback is one
  atomic file write plus one append-only journal line.

## Alternatives considered

- **Swap containers on deploy.** Rejected for the three reasons above.
- **Routing in redis.** Viable and equally atomic, but it adds a runtime
  dependency to the control plane and couples the deployer to a datastore the
  S3 scenario deliberately degrades. A file has no failure mode we need to
  scenario-test.
- **Routing via environment + restart of `orders`.** Rejected: a restart of
  the *routing* service to change the *routed* service is exactly the
  observability confound the benchmark does not need.

## Consequences

- The first seeded error appears **0.6 s** after the deploy journal entry; a
  rollback will be equally immediate. Verification can judge recovery on the
  next few windows rather than after a warm-up.
- The replay tool can target `payments-v1` and `payments-v2` by hostname with
  no change to live routing — replay is read-shaped.
- The Kubernetes delta is honest about this: there, routing is a Service
  selector or a weighted route, and a rollback is `kubectl rollout undo`. The
  idempotency key, TOCTOU check, and approval gate are unchanged because they
  are properties of the deployer tool, not the orchestrator.
- A mildly unrealistic property, stated plainly: real fleets rarely keep the
  previous version running. The demo keeps it because the *experiment* needs
  it; a production deployment would need a canary or a snapshot to replay
  against.

## Reversal conditions

If a scenario needs a version that cannot coexist with another (a schema
migration, say), that scenario gets a real stop/start deploy and its own
verification expectations — and this ADR gets an amendment, not a reversal.
