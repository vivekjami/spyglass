# Scenario ground-truth schema

Every scenario directory carries a `ground-truth.yaml` written **before** any
benchmark run and committed with the scenario. The benchmark scores an
investigation by joining the agent's cited evidence and terminal action against
this file. Nothing here is derived from a run; it is the pre-registered answer.

Validate with `just validate` (`scripts/validate-ground-truth.py`).

## Required top-level keys

| Key | Type | Meaning |
|---|---|---|
| `scenario` | string | Directory name, e.g. `s1-payment-regression` |
| `version` | int | Bump when the injected behaviour or noise profile changes |
| `description` | string | One sentence a human can check the run against |
| `seed` | int | `LOADGEN_SEED` the scenario is pinned to |
| `alert` | string | The first message of every investigation of this scenario — the same text for every condition (Phase 10) |
| `timeline` | map | What `inject.sh` does and in what order (see below) |
| `culprit` | map | The entity and change to blame — `service`, `change: {deploy_id, version, from_version}` (or `null` when the cause is not a change event), `mechanism` |
| `expected_evidence` | list | Evidence classes an ideal investigation cites; each has `kind` ∈ `novel_template \| burst_template \| changepoint \| deploy \| deploy_correlation \| metric_shift \| replay_separation \| exemplar \| verification \| absence`, `key` (counts toward recall), and a `match` map (below) unless `scored: false` |
| `decoys` | list | Things that look like evidence and are not — the noise the ranking must survive; each has `kind` and `note`, and a `match` map when a cited item can be joined to it |
| `correct_action` | map | `type` ∈ `rollback \| report_only \| refuse_escalate`; for `rollback`: `service`, `to_version` |
| `expected_error_rate` | map | `pre_fault_max` and `post_fault: {min, max}` — 5xx share of gateway checkouts; the reproducibility tolerances |
| `expected_latency` | map | optional: `pre_fault_p95_max_ms`, `post_fault_p95_min_ms` — for latency-shaped scenarios (S2, S6) |
| `verification_signal` | map | `metric` and `recovered_when` — what closes the incident |
| `scoring` | map | How `bench/report.py` reads the run: `verdict` (accepted `culprit_service` / `culprit_change` lists, `action`), `first_hypothesis_terms`, `decoy_terms` (below) |

## `timeline`

```yaml
timeline:
  steady_state_secs: 120          # traffic before anything happens
  benign_deploy:                  # optional decoy change
    service: orders
    version: v1.1
    deploy_id: D-1                # deterministic from clean state
    lead_secs_before_fault: 360
  fault:
    service: payments
    version: v2
    deploy_id: D-2
  post_fault_secs: 90             # how long inject.sh keeps observing
```

Deploy ids are deterministic because `just scenario` starts from a reset
journal and the deployer numbers only routing changes: the first is `D-1`, the
second `D-2`. Ground truth can therefore name the culprit change id up front.

## `match` — how a cited evidence id is joined to ground truth

The engine stamps every item it returns with an `eid` and a `kind`; the
benchmark runner keeps every tool response, so `bench/report.py` can resolve
each eid the agent cited to the item it named and test that item against the
`match` map of every expected-evidence and decoy entry. All listed fields must
hold:

| Field | Holds when |
|---|---|
| `kind` | the item's `kind` is in the list (`novel_template`, `template_hit`, `changepoint`, `deploy`, `replay_result`, `exemplar_request`, `verification_check`, …) |
| `pattern_contains` / `pattern_contains_any` | the template's `pattern` contains the text / any of the texts |
| `metric_in` | the changepoint's series metric (`error_rate`, `errors_total`, `latency_ms_mean`, …) is in the list |
| `service` | the item's service (or one of its `services`) equals it |
| `direction` | the changepoint's direction |
| `deploy_id` | the deploy item's id |
| `nearest_deploy_id` | the changepoint's `nearest_deploy.deploy_id` |
| `at_after_fault_secs: [min, max]` | the item's `at` (or `ts`) minus the run manifest's `t_fault`, in seconds, lies in the range |
| `replay_verdict` | the replay response's `comparison.verdict` |
| `origin_5xx_instance` | the exemplar's `outcome.origin_5xx` |

An item that matches an expected entry is *relevant*; one that matches a
decoy is a *decoy citation* (unless the decoy says `relevant: true` — a
symptom, or a change the report legitimately cites to rule out; blaming it
is caught by the verdict, not by the citation); anything else is *other*.

## `scoring` — the verdict block and the trace

Both SOPs end the report with a fenced `verdict` block (`culprit_service`,
`culprit_change`, `cause`, `action`, `evidence_label`). `scoring.verdict`
lists the accepted values; `none` is a value. `first_hypothesis_terms` is a
list of term groups — the first assistant message containing every term of
any group marks the time to first useful hypothesis. `decoy_terms` are counted
in interim assistant messages (decoy engagement).

## Scoring semantics

- **Root-cause accuracy**: the verdict's `culprit_service` and `culprit_change`
  are both accepted by `scoring.verdict` (top-1; `none` where the ground
  truth says there is no change to blame).
- **Evidence recall**: fraction of `key: true` expected-evidence entries with
  at least one cited item matching them.
- **Evidence precision**: relevant citations ÷ all cited eids (a cited decoy
  counts against precision, not just recall; `absence` entries are unscored).
- **Investigation success**: the terminal state equals `correct_action.type` —
  for `rollback`, exactly one rollback of the right service to the right
  version was executed; for `report_only` / `refuse_escalate`, nothing was
  executed and the verdict's `action` says so.
- **Verification success**: for rollback scenarios, recovery confirmed by
  telemetry before close — the engine's `verified_recovery` ledger entry
  (Spyglass), or a post-action metric re-check by the agent plus the runner's
  own post-run measurement under `pre_fault_max` (baseline).

## Run manifests

`inject.sh` writes `data/scenarios/<id>/<run>/manifest.json` with the absolute
timestamps (`t_start`, `t_benign_deploy`, `t_fault`) and the deployer journal
entries it produced, plus a snapshot of the logs. Ground truth is relative;
manifests are absolute; the join is the deploy id.
