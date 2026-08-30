# scenarios — the incident corpus

One directory per fault, each with pre-registered ground truth
([`SCHEMA.md`](SCHEMA.md)), an injector, a noise profile and a README with
the measured acceptance (two runs from clean state, same curve). The corpus
is a first-class artifact ([ADR-015](../docs/adr/ADR-015-scenario-corpus-and-bench-are-durable.md)):
`bench/report.py` scores every investigation against these files and
nothing else.

| ID | Directory | The cause | The evidence | Correct outcome | Status |
|---|---|---|---|---|---|
| S1 | [`s1-payment-regression/`](s1-payment-regression/) | `payments v2` (`D-2`) raises on non-USD charges | novel ERROR template with a stack at the culprit; error changepoint +0.6 s after the deploy; replay separates v1 from v2 | rollback payments → v1 | ✓ Phase 1, ground truth v2 (Phase 10) |
| S2 | [`s2-timeout-cascade/`](s2-timeout-cascade/) | `orders v1.2` (`D-1`), a config-only release: vendor API v2 + doubled timeout | latency cascade orders → gateway → edge 5xx; the change event; **no** template at the culprit; a gateway blip as decoy | rollback orders → v1 | ✓ Phase 10 |
| S3 | [`s3-redis-pressure/`](s3-redis-pressure/) | a 66 MB blob fills the shared redis (`noeviction`); no change event | a known-but-rare template bursting ~100× at ERROR; the store's own memory numbers; no deploy correlation | report-only (nothing to roll back) | ✓ Phase 10 |
| S4 | — | connection-pool leak, slow drift | CUSUM territory | report + escalate | ○ not built (drop order) |
| S5 | — | one of three replicas misconfigured | per-instance delta | report + escalate | ○ not built (drop order) |
| S6 | [`s6-insufficient-evidence/`](s6-insufficient-evidence/) | the unobserved fraud vendor degrades; no change event | a latency changepoint at orders and *nothing else*; a benign deploy six minutes earlier as the tempting rollback | refuse to act; say what evidence would decide it | ✓ Phase 10 |

```
SCENARIO_FAST=1 just scenario s2      # clean state, stack up, engines restarted, fault injected and left active
just scenario-check s2                # the last two runs against the ground truth's tolerances
just validate                         # every ground-truth.yaml against SCHEMA.md
```

Every scenario shares the seeded traffic and background noise of S1
(`noise.yaml` in each directory says what differs). Faults that are not
change events are injector steps against the environment — a redis command,
a `/knobs` file — and their ground truth says `change: null`.
