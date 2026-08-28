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
| `timeline` | map | What `inject.sh` does and in what order (see below) |
| `culprit` | map | The entity and change to blame — `service`, `change: {deploy_id, version, from_version}`, `mechanism` |
| `expected_evidence` | list | Evidence classes an ideal investigation cites; each has `kind` ∈ `novel_template \| changepoint \| deploy \| deploy_correlation \| metric_shift \| absence` |
| `decoys` | list | Things that look like evidence and are not — the noise the ranking must survive; each has `kind` and `note` |
| `correct_action` | map | `type` ∈ `rollback \| report_only \| refuse_escalate`; for `rollback`: `service`, `to_version` |
| `expected_error_rate` | map | `pre_fault_max` and `post_fault: {min, max}` — 5xx share of gateway checkouts; the reproducibility tolerances |
| `verification_signal` | map | `metric` and `recovered_when` — what closes the incident |

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

## Scoring semantics

- **Root-cause accuracy**: agent's blamed `{service, change}` equals `culprit`.
- **Evidence recall**: fraction of `expected_evidence` kinds the agent cited.
- **Evidence precision**: fraction of cited eids that map to expected evidence
  (a cited decoy counts against precision, not just recall).
- **Investigation success**: terminal state equals `correct_action.type`
  (and, for rollback, the right service/version) *and* — where applicable —
  `verification_signal` was observed before close.

## Run manifests

`inject.sh` writes `data/scenarios/<id>/<run>/manifest.json` with the absolute
timestamps (`t_start`, `t_benign_deploy`, `t_fault`) and the deployer journal
entries it produced, plus a snapshot of the logs. Ground truth is relative;
manifests are absolute; the join is the deploy id.
