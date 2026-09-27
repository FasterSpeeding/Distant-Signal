use clap::Parser;

/// CLI/env configuration for the `enricher` service.
#[derive(Debug, Parser)]
pub struct Config {
    #[arg(long, env)]
    pub database_url: String,

    #[arg(long, env)]
    pub redis_url: String,

    /// Base URL of an OpenAI-compatible Chat Completions endpoint, e.g.
    /// `http://localhost:8080/v1` for a local server. No vendor is assumed.
    #[arg(long, env)]
    pub llm_base_url: String,

    /// Optional -- many local OpenAI-compatible servers don't require one.
    #[arg(long, env)]
    pub llm_api_key: Option<String>,

    /// Model name/identifier as the endpoint expects it.
    #[arg(long, env)]
    pub llm_model: String,

    /// Per-request timeout for a single LLM call. One incident makes three
    /// sequential calls (primary, resolution-adversarial, severity-adversarial
    /// -- see `llm.rs`), so the worst case for one incident is roughly 3x
    /// this value. Real self-hosted endpoints vary widely in latency; raise
    /// this if extractions are timing out against a slow/remote server, but
    /// raise `reclaim_min_idle_secs` to match (see its doc comment).
    ///
    /// Raised from 120 to 300 (2026-08-21) after a live eval against a real
    /// self-hosted endpoint (Ollama, qwen3.5:4b) measured single-call
    /// latencies of 86-104s for the *flat* single-period case alone -- within
    /// striking distance of the old 120s default, and multi-period payloads
    /// (larger prompt/response) push that further. See
    /// docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md.
    #[arg(long, env, default_value_t = 300)]
    pub llm_request_timeout_secs: u64,

    // --- PROTOTYPE (research-nvidia-llm, 2026-09-27): per-provider knobs
    // for a slow, rate-limited hosted endpoint (NVIDIA free tier). All
    // default to "off" so an unset deployment behaves exactly as before.
    // Set via `enricher.extraEnv` in the chart for a trial -- no template
    // change needed. See llm::ProviderPolicy.
    /// Sent as `max_tokens` when set (e.g. 8192 for GLM-5.3).
    #[arg(long, env)]
    pub llm_max_tokens: Option<u32>,
    /// Sent as `reasoning_effort` when set (e.g. `low` for GLM-5.3).
    #[arg(long, env)]
    pub llm_reasoning_effort: Option<String>,
    /// Cap on concurrent LLM HTTP attempts across stream/sweep/reclaim.
    #[arg(long, env)]
    pub llm_max_in_flight: Option<usize>,
    /// Minimum 429 back-off before an in-call retry.
    #[arg(long, env, default_value_t = 20)]
    pub llm_rate_limit_retry_secs: u64,
    /// In-call retries on 429 (0 = fail the incident, today's behaviour).
    #[arg(long, env, default_value_t = 0)]
    pub llm_rate_limit_retries: u32,
    /// In-call retries on 502/503/504/client timeout (0 = today's behaviour).
    #[arg(long, env, default_value_t = 0)]
    pub llm_gateway_retries: u32,

    /// How often the reconciliation sweep runs, independent of the Redis
    /// Stream consumer loop. Backstop for a missed/lost publish.
    #[arg(long, env, default_value_t = 3600)]
    pub sweep_interval_secs: u64,

    /// How often to check the Redis Stream consumer group's pending-entries
    /// list for entries stuck longer than `reclaim_min_idle_secs` -- the
    /// debounced retry path for a request that timed out, or a process that
    /// crashed between processing and acking.
    #[arg(long, env, default_value_t = 60)]
    pub reclaim_interval_secs: u64,

    /// How long a pending entry must have sat unacked before it's eligible
    /// for reclaim. Must comfortably exceed the worst-case time to run all
    /// three extraction calls (each bounded by `llm_request_timeout_secs`)
    /// plus the DB write, so a still-in-flight attempt is never reclaimed
    /// out from under itself -- if you raise `llm_request_timeout_secs`,
    /// raise this too (default here is set for the default 300s timeout:
    /// 3 * 300s = 900s worst case, plus headroom).
    #[arg(long, env, default_value_t = 1000)]
    pub reclaim_min_idle_secs: u64,

    /// Port for this service's Prometheus `/metrics` endpoint. See
    /// docs/superpowers/plans/2026-08-29-metrics.md's Global Constraints
    /// for why this differs from api.service.port -- api reuses its
    /// existing HTTP listener, this service has none, so it needs a new one.
    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,

    /// Whether to start this service's Prometheus `/metrics` listener at
    /// all. Distinct from `metrics_port` (which port to use IF started) --
    /// this is what actually satisfies "metrics.enabled=false leaves the
    /// service working exactly as it does today" (see the Helm chart's
    /// `metrics.enabled` value and this branch's final whole-branch
    /// review, Important finding #2): omitting the containerPort/env/
    /// annotations in the chart alone does not stop the process from
    /// listening, since Kubernetes container ports are purely
    /// declarative.
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,

    /// RESEARCH PROTOTYPE (diff-aware enricher, option a). When true, a
    /// text change that `text_delta::classify` judges a semantic no-op
    /// (HTML/whitespace/entity/case/in-word-punctuation only) re-stamps the
    /// existing extraction's `source_text_hash` instead of running the LLM.
    /// Off by default: with it off, the only behavior change is the extra
    /// `edit_class` label on the churn metric (measurement only).
    #[arg(long, env, default_value_t = false)]
    pub carry_forward_semantic_noops: bool,
}
