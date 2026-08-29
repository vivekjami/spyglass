# bench — the benchmark harness

The measurement infrastructure is the asset that compounds ([ADR-015](../docs/adr/ADR-015-scenario-corpus-and-bench-are-durable.md)):
conditions, a runner, a results format and a scorer that reads pre-registered
ground truth. Nothing in here knows what the answer should be except the
scenario's `ground-truth.yaml`, committed before any benchmark run.

```
bench/
├── conditions/        one TrueForge agent manifest per condition + the fairness checklist (README)
├── run.py             the runner: {conditions} x {scenarios} x repeats, unattended
├── report.py          the scorer: run files + ground truth -> docs/benchmark.md and README tables
├── price-sheet.json   provider prices per 1M tokens at run time (null = cost column reads n/a)
└── results/           one JSON per investigation, every run ever made, failures included
```

## One run

`scripts/investigate.py --condition <c> --scenario <s> --approval allow --bench`
opens a session on the condition's named agent, posts the scenario's alert
(from its ground truth), drives the approval gate (`allow` for unattended
runs: the human is simulated as approving, so a wrong proposal executes and
is scored as a wrong action), and writes `bench/results/<s>-<c>-<run>.json`:

| Key | What |
|---|---|
| `outcome`, `valid`, `invalid_reason` | `completed`, or `harness_error` / `max_turns` (kept, `valid: false`, excluded from aggregates) |
| `metrics` | wall time, turns, tool calls (by name), model calls, threads, input / output / cache-read tokens, tool bytes returned to the context, approvals, rollbacks executed, journal entries added, versions before/after, the runner's own edge measurement before and after (5xx share and p95 latency over 10 s / 20 s), sandbox `exec` commands |
| `verification` | the engine's verdict (Spyglass): checks, closed / escalated, the closing summary |
| `ledger` | the investigation's ledger entries, eids issued, eids cited in the RCA, the digest re-check verdict |
| `scenario_run` | the injector's manifest (absolute `t_fault` etc.) so scoring needs nothing outside this file |
| `provenance` | condition file and SOP file hashes |
| `final_output`, `turns`, `events` | the RCA and the full event trace — every number is traceable |

## The matrix

`bench/run.py` (`just bench`) runs one fresh incident per cell — `just scenario
<s>` on the fast timeline (clean state, stack up, evidence engines restarted so
their history is the scenario's alone, fault injected and left active), then
one investigation — and re-runs a cell whose run was invalid (the invalid file
stays). Default order: the pre-agreed floor first (S1–S3 × {baseline,
spyglass}, one repeat of every cell before the next), then S6, then the
ablation. `--dry-run` prints the plan; `--scenarios`, `--conditions`,
`--repeats` narrow it. The script commits nothing: commit `bench/results/`
yourself, every file.

## Scoring

`bench/report.py` (`just report`) joins each run to its scenario's ground
truth ([`scenarios/SCHEMA.md`](../scenarios/SCHEMA.md), *Scoring semantics*):

- **success** — terminal state equals `correct_action` (the right rollback
  executed exactly once; or nothing executed and the closing `verdict` block
  says the right report / refusal);
- **RCA correct** — the verdict's `culprit_service` and `culprit_change` are
  both accepted; no verdict block, not correct;
- **evidence precision / recall** — every cited evidence id resolved to the
  item the engine returned (the trace keeps every tool response) and matched
  against the ground truth's `match` maps; Spyglass conditions only — the
  baseline has no evidence ids, which is a finding;
- **verified** — rollback scenarios: the engine's `verified_recovery` entry
  (Spyglass) or the agent's own post-action metric re-check plus the runner's
  post-run 5xx under `pre_fault_max` (baseline);
- tokens, tool calls, alert→RCA seconds, alert→verified-close seconds, time
  to first useful hypothesis, decoy mentions in interim messages, cost from
  the price sheet.

Only runs with `"benchmark": true` enter the tables; earlier per-phase runs
stay in the preliminary section of `docs/benchmark.md`. `--all` scores them
too (development).
