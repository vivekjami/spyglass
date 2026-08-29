# Benchmark conditions

One JSON file per condition. Each is a TrueForge agent manifest plus two
conveniences the setup script resolves: `instructions_file` (the SOP text
lives in `agent/` where it can be reviewed as prose) and `"$MODEL_A"` /
`"$MODEL_B"` (resolved against `.env` and what the harness actually serves).

`scripts/tf-setup.py` registers the MCP servers and every condition as a named
TrueForge agent. `scripts/investigate.py --condition <name>` runs one
investigation and writes its metrics to `bench/results/`.

## The fairness checklist (ADR-012)

The claim is about *evidence*, so everything else is held constant. Any
asymmetry here invalidates the result. Checked per condition file:

| Held constant | How |
|---|---|
| **Model** | `"$MODEL_A"` in every condition; resolved to the same catalog entry |
| **Harness settings** | `config` block pinned explicitly and identically: `iteration_limit`, sandbox, sub-agents, `compaction: off`, `large_tool_response: off` (ADR-016), `preload: true` on every MCP server (deferred-loading calls would otherwise land unevenly on the tool-call metric) |
| **Information access** | Both conditions read the same files and endpoints. Mapping below. |
| **Action path** | The *same* `spyglass-deployer` MCP server, the same `rollback` tool, the same `require_approval_for_tools: ["rollback"]` gate, the same journal |
| **Alert** | The same first message, from the same watcher |
| **What differs** | Only the read tools: raw (`tail`/`grep`/`curl`-shaped) vs. shaped (templates, novelty, changepoints, ranking, bundles, evidence ids) |

### Information-access mapping

| Underlying data | Baseline sees it via | Spyglass sees it via |
|---|---|---|
| `data/logs/<instance>.jsonl` | `tail_logs`, `grep_logs` (regex, window, limit ≤ 1000, truncation reported) | `search_logs`, `novel_templates`, `get_evidence` (bounded, ranked, deduped) |
| Request outcomes (status, latency per line in the logs; the same facts the `/metrics` counters aggregate) | `tail_logs`, `grep_logs` on the request lines; `get_metric` (raw Prometheus text; call twice for a rate) | `detect_changepoints`, `error_delta` (computed from the same request lines — the detector reads the logs, not the scraper, so its output re-checks; ADR-007) |
| `data/deploy/journal.jsonl` | `deploy_events` (verbatim) | `deploy_events` (verbatim, plus correlation annotations elsewhere) |
| Topology | `list_services` (from the compose layout) | `service_topology` |
| Routing state | `current_versions` | `current_versions` |
| Captured requests | in the gateway log (`kind=request_capture`), via grep | `get_exemplar_request` |

The baseline's tools have limits because real tools do (`tail -n`, `grep |
head`); they are generous and every truncated response states the total, so
the agent can page. What the baseline does *not* get is any ranking, novelty,
dedup, or evidence id — that is the treatment.

### What is deliberately NOT tuned

The baseline SOP is a competent on-call prompt, not a strawman: a method, the
tool list, "be thorough", and the data-not-instructions rule. It gets no hints
about which log lines matter. If the baseline finds the root cause quickly,
that is a result, and it is reported.
