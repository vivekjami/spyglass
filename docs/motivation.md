# Motivation — what we are building, and why

> One paragraph, if that is all you read: AI agents are measurably bad at
> production incident investigation, and the published failure mode is not
> "the model cannot reason" — it is that the model drowns in telemetry.
> Spyglass tests whether fixing the *evidence* fixes the *agent*, by holding
> the model constant and varying only what it is shown.

## 1. The problem, as measured rather than asserted

Incident investigation is a real, expensive, paged-human problem, and it is one
where agents currently underperform badly:

- On **ITBench-AA** (Artificial Analysis × IBM, May 2026), *every* evaluated
  frontier model scored **below 50%** on Kubernetes incident root-cause tasks.
- The documented failure mode was **over-investigation**: agents that page
  through raw telemetry surface co-occurring symptoms and fault-injection
  mechanics as false positives — and **longer trajectories did not improve
  accuracy**.
- The original **ITBench** paper (arXiv:2502.05352) measured state-of-the-art
  agents resolving only **~13.8%** of SRE scenarios.

That last bullet is the one that shapes this project. If more steps and more
tokens made agents better at this, the fix would be patience and budget. They
don't. So something upstream of the reasoning is wrong.

## 2. What actually goes wrong

A production system under incident emits far more evidence than any context
window can absorb: thousands of log lines per minute (mostly repetition),
hundreds of metric series (most moving for unrelated reasons), request paths,
deploy events, config changes, alerts, and historical baselines.

Hand an agent unrestricted access to that — `tail_logs`, `grep`, raw metric
dumps — and the pathologies are predictable, and each is measurable in this
project's baseline condition:

| Pathology | Mechanism | Cost |
|---|---|---|
| Over-investigation | no ranking signal, so the agent keeps pulling more raw data | tool calls, latency |
| Token blowout | raw log pages are ~95%+ repetition of known-normal templates | direct model cost |
| Irrelevant evidence | during an incident, everything correlates with everything | wrong hypotheses |
| Stale evidence | the agent cannot tell how fresh its view is | confident wrong answers |
| Bad causal inference | "deployed 2 min before errors" treated as cause | wrong remediation |
| Context pressure | early evidence evicted or compacted mid-investigation | lost reasoning threads |
| Hallucinated conclusions | under weak evidence, models complete the pattern anyway | trust destroyed |

## 3. The bet

> **Incident investigation is not primarily a reasoning problem. It is an
> evidence problem. Do not try to make the model smarter — make the evidence
> presented to the model better.**

The mechanism behind the bet: incident-relevant evidence is **sparse and
change-shaped**. What matters is what is **new** (a never-before-seen log
template), what **changed** (an error-rate step, a latency changepoint), and
what **coincided with a change event** (a deploy, a config flip).

Those properties are computable *before the model sees a single byte* —
cheaply, deterministically, in Rust. Computing them upstream converts an
open-ended reading-comprehension task into a short structured-reasoning task.

## 4. Why an evidence *plane*, and not the obvious alternatives

Three cheaper things could be tried first. Each was considered and rejected for
a specific reason, recorded so the choice is arguable rather than assumed:

- **Better prompts over raw tools.** A prompt cannot bound a context window and
  cannot rank 184,000 events. "Please be brief" is not an enforcement
  mechanism; an engine-side item/byte cap is. (ADR-001, ADR-005)
- **A bigger context window.** Cost scales with the garbage as well as the
  signal, and ITBench-AA already showed longer trajectories don't improve
  accuracy. Paying more to be wrong more slowly is not a fix. (ADR-001)
- **Fine-tuning a model on incidents.** Off-thesis by definition, data-hungry,
  and unexplainable. The claim here is about evidence, not weights. (ADR-001)

The observability industry already solved the *human* version of this problem —
dashboards, alert routing, SLO burn rates are all evidence shaping for
eyeballs. The equivalent layer **for agents** — shaped for token budgets, ranked
by novelty and change, bounded by construction, auditable after the fact — does
not exist in open source today:

- **HolmesGPT** (the leading CNCF-sandbox open-source incident agent) queries
  existing observability backends via toolset passthrough; its own CNCF filing
  lists log-volume reduction and anomaly detection as *future plans*.
- The official **Grafana MCP server** exposes PromQL/LogQL passthrough — the
  model writes query-language strings and pages raw results.

That gap is what Spyglass occupies.

## 5. Correlation is not enough — the second bet

Even perfect evidence ranking yields only *"payments v2 was deployed 118
seconds before the error-rate changepoint."* That is a correlation. Co-deploys,
coincident traffic shifts, and upstream faults all produce the same picture, and
acting on it is exactly how automated remediation earns its bad reputation.

So Spyglass adds a step the product category has not: it **replays the captured
failing request** against the suspected-bad and known-good versions in a
sandbox. Same input, versions varied, outcome measured — a controlled
experiment, in the middle of an investigation. Correlation becomes causal
evidence, or the hypothesis dies. (ADR-010)

## 6. What would prove this wrong

The thesis is stated so it can lose:

- **Hold constant:** model, harness, incident, information access, action path.
- **Vary only:** the evidence interface. Baseline gets raw telemetry tools;
  treatment gets the Spyglass evidence plane.
- **Predict:** treatment finds the correct root cause at least as often, with
  materially fewer tool calls, fewer tokens, lower cost, and lower wall-clock
  time — and its conclusions carry evidence citations the baseline cannot
  produce.

**If the prediction fails, the benchmark reports that.** Every run is committed,
including failures. A negative or mixed result is a finding and is published as
one — the alternative is an unfalsifiable demo, which is worth nothing.

Two things would falsify or badly weaken the thesis, and we would say so:

1. Shaped evidence shows **no material gain** over raw tools under identical
   conditions → ADR-001 is wrong, and the writeup says the evidence plane did
   not earn its complexity.
2. The gain appears **only under one model** → the improvement was partly
   prompt-idiosyncratic rather than structural. The engine runs *before* the
   context window, so a model-agnostic benefit is the prediction; if the
   generalization cells contradict it, that is reported.

## 7. What this project is not

It is a hackathon technical demonstration, not a product launch. It does not
replace Prometheus, Grafana, or Loki — it is an evidence layer over telemetry
those systems already store. It does not do autonomous remediation: the agent
proposes, a human approves, and exactly one mutating action exists. And it makes
no commercial claims — see the root README's Enterprise Relevance section, where
every sentence is deliberately "could," never "does."

## Sources

- ITBench: *Evaluating AI Agents across Diverse Real-World IT Automation Tasks* — arXiv:2502.05352
- ITBench-AA (Artificial Analysis × IBM, May 2026) — huggingface.co/blog/ibm-research/itbench-aa
- HolmesGPT — github.com/HolmesGPT/holmesgpt; CNCF sandbox application (cncf/sandbox #392)
- Grafana MCP server — grafana.com/docs/grafana/latest/developer-resources/mcp/
- Drain log parsing: He et al., *Drain: An Online Log Parsing Approach with Fixed Depth Tree*, ICWS 2017
