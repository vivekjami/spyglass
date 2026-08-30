# Analyst briefs

TrueForge sub-agents are dynamic: the lead calls `create_sub_agent(name,
input)` and writes the whole brief as `input` (Phase 0, F11). These files are
the briefs the SOP tells the lead to generate — kept here as prose so they can
be reviewed, and pasted into `agent/sop.md` in compact form.

The fan-out is **conditional** (SOP step 3): it runs only when the bundle
leaves a hypothesis unresolved. On S1 the bundle's head names the template,
the changepoint and the deploy of one service within a second of each other,
so no analyst is spawned — measured in Phase 7, a fan-out there would add
tokens and no facts. Budgets are advisory text (there is no per-sub-agent
limit in the harness); the real limits are `config.iteration_limit` and the
engine's bounds.

| Brief | Question it answers | Tools it may use |
|---|---|---|
| [`logs-analyst.md`](logs-analyst.md) | what is new in the logs, and does it fit the hypothesis | `novel_templates`, `get_evidence`, `search_logs` |
| [`metrics-analyst.md`](metrics-analyst.md) | what changed first, in which service, how far from a deploy | `detect_changepoints`, `error_delta` |
| [`change-analyst.md`](change-analyst.md) | what was changed, and can the timing explain it | `deploy_events`, `error_delta`, `current_versions` |

Every analyst returns *findings with evidence ids* and a verdict — supports /
contradicts / undetermined — and never acts.
