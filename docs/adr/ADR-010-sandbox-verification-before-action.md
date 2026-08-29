# ADR-010 — A controlled replay before any action

**Status:** Accepted, amended · **Date:** 2026-08-28 (expanded and amended at Phase 8, when the causal check was built)

## Context

Correlation is cheap and wrong often enough to matter. "payments v2 was
deployed 0.6 s before the error-rate changepoint" is a true, structured,
engine-computed fact — and co-deploys, coincident traffic shifts and
upstream faults all produce the same picture. An agent that acts on it is
an agent that will one day roll back the wrong service with a confident
postmortem.

The spec's remedy was an experiment: take the request a client actually
sent, replay it N times against the suspected-bad and the known-good
version, and compare failure proportions. Same input, versions varied,
outcome measured. The spec placed that experiment in the TrueForge sandbox,
with agent-written code as the executor.

## Decision

1. **A deploy-shaped hypothesis must attempt an exemplar replay before any
   action proposal.** The SOP's ACT exit requires a `separated` replay when
   a replay was possible; when it was not (no version pair, no captured
   request), acting is allowed on a coherent, uncontradicted deploy
   hypothesis *stated as correlational, with the reason the replay could
   not run*. A replay that fails on every version rejects the hypothesis.
2. **The executor is the evidence engine, not the sandbox** — the Phase 0
   fallback (A). Two tools on the engine: `get_exemplar_request` (one
   captured request, sanitized, with its chain through the services and
   the 5xx origin) and `replay_exemplar` (N replays per version straight to
   the always-on instances' published ports, proportions side by side, a
   stated threshold, a verdict and a reading). The agent still designs the
   experiment — which exemplar, which service, which versions, N — and
   receives the proportions as evidence ids it must cite.
3. **The experiment is bounded and is not evidence of itself.** `n` is
   clamped, every request times out, bodies are capped and sanitized before
   they are sent anywhere, live routing is never touched, and every replay
   carries a `replay-*` request id that the tailer drops — the engine's own
   traffic never moves a count, a rate, a watermark, or a template.
4. **Causal language is earned, not assumed.** The tools' vocabulary is
   fixed: `separated` = "for this request class the failure is a property
   of version B — causal evidence for THIS failure mode, not proof it is
   the only one"; `not_separated` names which way (no version / every
   version / partial) and forbids "caused". No p-values at N = 20; raw
   proportions are reported and the threshold is in config.

## Why the executor moved (Phase 0, F9)

TrueForge's local sandbox removes the network namespace, sets `NO_PROXY` to
every private range, and its egress allowlist is a hard-coded constant
(pypi, github). Agent-written code in it cannot reach the Compose network,
and there is no supported knob. Patching the harness bundle would not
reproduce on a judge's machine and is the "rebuild sponsor infrastructure"
anti-pattern the spec rules out. The controlled experiment survives; its
executor changes. What is lost is the "agent-written code runs in the
sponsor's sandbox" story; the sandbox keeps its other job in the SOP (the
verification `sleep`, and any analysis over ledger data).

## What the experiment does and does not show (measured, Phase 8)

On S1 the first non-USD checkout that met `payments v2` — captured by the
gateway 18 ms before `payments-v2` raised — replayed 20× per version:
**v1 0/20, v2 20/20, Δ 1.00, `separated`**, in under a second. The same
tool on a request that succeeded: **0/20 vs 0/20, `not_separated`** — the
tool can say no, and the SOP treats that as "this exemplar does not
reproduce the failure", not as absolution. The experiment's 80 requests
produced 80 log lines; the engine excluded exactly 80 and `payments-v1`
showed zero requests in the replay window.

Limits, stated on every result and in the RCA template: one exemplar
class; deterministic bugs separate cleanly, load-dependent ones (S4) may
not at N = 20 and then the SOP reports correlational confidence rather
than manufacturing certainty.

## Alternatives considered

- **Act on correlation; rollback is cheap.** Rejected: it normalises
  exactly the behaviour the safety model exists to prevent, and forfeits
  the project's central demo moment.
- **Canary in production** (route a share of live traffic to each version
  and compare). Rejected: mutates live routing, which is the one thing the
  agent may only do behind the gate.
- **Daytona sandbox + a public tunnel** (fallback B). Rejected for the
  hackathon: three external dependencies live during a demo, to keep a
  story rather than an experiment.
- **Deploy-window bisection with a correlational RCA** (fallback C, the
  spec's own). Strictly weaker — it gives up causal evidence — so it is the
  fallback to this one, and the SOP's "replay not possible" path is it.
- **A third "experiment" MCP server.** Rejected: evidence ids are issued per
  engine session; a replay on another server could not land as `E5`/`E6`
  in the same investigation without plumbing that does not exist.

## Consequences

- The engine's read plane gains one tool that touches the world: bounded
  synthetic traffic to always-on instances. `docs/safety.md` records its
  side effects (the instances' `/metrics` counters and the payments cache
  see it) and what keeps it out of the evidence.
- Investigations gain two tool calls and about a second — a price the
  benchmark reports rather than hides.
- The baseline gets the raw counterpart, `http_request` (one request, like
  `curl`), so the comparison stays "shaped vs raw", not "can vs cannot".
- The ranking's post-hoc causal boost from the spec is **not built**: the
  replay's eids are cited directly, and a second bundle after the replay
  would not change what the agent does. Recorded so it is not mistaken for
  an omission.

## Reversal conditions

If a scenario shows the exemplar class replayed is systematically the
wrong one (the first failing request is not representative), add
multi-exemplar classes — deferred by the spec. If the harness gains a
supported egress allowlist, the executor can move back into the sandbox
with the tool contract unchanged: the SOP would call the same two steps.
