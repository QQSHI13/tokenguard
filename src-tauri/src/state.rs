//! Global application state shared between the Tauri shell and the proxy.

use crate::config::{
    Config, Limit, LimitAction, LimitGroup, LimitMetric, LimitScope, Project, Provider,
    ProviderFormat,
};
use crate::db::{self, DbPool};
use crate::health::{HealthCache, ProviderHealth};
use crate::limits::LimitCounters;
use crate::webhook;

/// Pure routing logic, extracted for unit testing.
/// Routing is model-only: any provider that supports the requested model can
/// serve the request, regardless of the client API format. Conversion happens
/// downstream between the client format and the provider format.
pub fn route_in_list(
    providers: &[Provider],
    _family: ProviderFormat,
    model: &str,
) -> Option<Provider> {
    if !model.is_empty() {
        if let Some(p) = providers
            .iter()
            .find(|p| p.models.iter().any(|m| m.local == model))
            .cloned()
        {
            return Some(p);
        }
    }
    providers
        .iter()
        .find(|p| p.is_default)
        .cloned()
        .or_else(|| providers.first().cloned())
}

/// Find the remote model name for a given local model name on a provider.
/// Returns the local name as fallback if no mapping exists.
pub fn remote_model_name(provider: &Provider, local_name: &str) -> String {
    provider
        .models
        .iter()
        .find(|m| m.local == local_name)
        .map(|m| m.remote.clone())
        .unwrap_or_else(|| local_name.to_string())
}

/// Return the full pricing profile override for a given local model name on a
/// provider, or an empty profile if the model has no mapping.
pub fn pricing_profile(provider: &Provider, local_name: &str) -> crate::cost::PricingProfile {
    provider
        .models
        .iter()
        .find(|m| m.local == local_name)
        .map(|m| m.pricing.clone())
        .unwrap_or_default()
}

use chrono::{Datelike, Timelike};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Pure scope-matching logic, extracted for unit testing.
pub fn limit_scope_matches(
    limit: &Limit,
    provider_id: i64,
    project_tag: Option<&str>,
    model: Option<&str>,
    projects: &[Project],
) -> bool {
    let scope_ok = match limit.scope {
        LimitScope::Global => true,
        LimitScope::Provider => limit.scope_id == Some(provider_id),
        LimitScope::Project => {
            if let Some(pid) = limit.scope_id {
                projects
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| project_tag == Some(p.name.as_str()))
                    .unwrap_or(false)
            } else {
                false
            }
        }
        LimitScope::Model => true,
    };
    if !scope_ok {
        return false;
    }

    let pattern = normalized_pattern(limit.model_pattern.as_deref());

    // Model-scope limits only apply when the requested model matches. A blank
    // pattern would match nothing, so the limit would silently enforce nothing;
    // treat it as match-all instead. New limits are rejected at creation
    // (commands::validate_model_pattern), so this only rescues rows written
    // before that check existed.
    if limit.scope == LimitScope::Model {
        return match pattern {
            None => true,
            Some(p) => model_pattern_matches(Some(p), model),
        };
    }

    // Any scope can optionally be narrowed to a model pattern.
    if let Some(p) = pattern {
        return model_pattern_matches(Some(p), model);
    }

    true
}

/// A pattern that is absent or blank means "no model narrowing" — blank strings
/// arrive from UI fields the user left empty.
fn normalized_pattern(pattern: Option<&str>) -> Option<&str> {
    pattern.map(str::trim).filter(|p| !p.is_empty())
}

fn model_pattern_matches(pattern: Option<&str>, model: Option<&str>) -> bool {
    match (pattern, model) {
        (None, _) => false,
        (Some(_), None) => false,
        (Some(p), Some(m)) => m.to_lowercase().contains(&p.to_lowercase()),
    }
}

fn limit_scope_matches_group(
    group: &LimitGroup,
    provider_id: i64,
    project_tag: Option<&str>,
    model: Option<&str>,
    projects: &[Project],
) -> bool {
    let scope_ok = match group.scope {
        LimitScope::Global => true,
        LimitScope::Provider => group.scope_id == Some(provider_id),
        LimitScope::Project => {
            if let Some(pid) = group.scope_id {
                projects
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| project_tag == Some(p.name.as_str()))
                    .unwrap_or(false)
            } else {
                false
            }
        }
        LimitScope::Model => true,
    };
    if !scope_ok {
        return false;
    }

    let pattern = normalized_pattern(group.model_pattern.as_deref());

    if group.scope == LimitScope::Model {
        return match pattern {
            None => true,
            Some(p) => model_pattern_matches(Some(p), model),
        };
    }

    if let Some(p) = pattern {
        return model_pattern_matches(Some(p), model);
    }

    true
}
use crate::notifier::Notifier;
#[cfg(feature = "gui")]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
#[cfg(feature = "gui")]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(feature = "gui")]
use tauri::{AppHandle, Emitter, Manager, Wry};

#[cfg(feature = "gui")]
const ICON_GREEN: &[u8] = include_bytes!("../icons/icon_green.png");
#[cfg(feature = "gui")]
const ICON_YELLOW: &[u8] = include_bytes!("../icons/icon_yellow.png");
#[cfg(feature = "gui")]
const ICON_ORANGE: &[u8] = include_bytes!("../icons/icon_orange.png");
#[cfg(feature = "gui")]
const ICON_RED: &[u8] = include_bytes!("../icons/icon_red.png");

#[derive(Debug, Clone)]
pub struct LimitViolation {
    pub limit: Limit,
    pub used: f64,
    /// Whether this violation should trigger a desktop notification, based on
    /// a non-mutating cooldown peek. The caller records the notification via
    /// mark_block_notified()/mark_warning_notified() when it actually notifies.
    pub should_notify: bool,
}

#[derive(Debug)]
pub struct GroupViolation {
    pub group: LimitGroup,
    pub used: f64,
    pub should_notify: bool,
}

/// Result of a limit check. Reservations made in the in-flight counters are
/// returned so the caller can release each exactly once per terminal outcome:
/// immediately on block/pause, or after the request is logged on success.
#[derive(Debug, Default)]
pub struct LimitCheckResult {
    pub violations: Vec<LimitViolation>,
    pub group_violations: Vec<GroupViolation>,
    /// Limit ids and the amounts reserved in the in-flight counters.
    pub reservations: Vec<(i64, f64)>,
    /// Group ids and the amounts reserved in the in-flight group counters.
    pub group_reservations: Vec<(i64, f64)>,
}

const WARNING_COOLDOWN: Duration = Duration::from_secs(300);

/// The parts of a violation the proxy has to act on. Individual limits and limit
/// groups differ only in wording, so both collapse to this and share one
/// handler instead of four near-identical match arms.
#[derive(Debug)]
pub struct ViolationAction {
    pub id: i64,
    pub name: String,
    pub action: LimitAction,
    pub used: f64,
    pub cap: f64,
    pub should_notify: bool,
    /// "limit" or "limit group" — used in the client-facing message.
    pub kind: &'static str,
}

impl From<&LimitViolation> for ViolationAction {
    fn from(v: &LimitViolation) -> Self {
        Self {
            id: v.limit.id,
            name: v.limit.name.clone(),
            action: v.limit.action,
            used: v.used,
            cap: v.limit.cap,
            should_notify: v.should_notify,
            kind: "limit",
        }
    }
}

impl From<&GroupViolation> for ViolationAction {
    fn from(v: &GroupViolation) -> Self {
        Self {
            id: v.group.id,
            name: v.group.name.clone(),
            action: v.group.action,
            used: v.used,
            cap: v.group.cap,
            should_notify: v.should_notify,
            kind: "limit group",
        }
    }
}

/// When a violation is being handled relative to the request that triggered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitPhase {
    /// Before forwarding: the request can still be refused.
    PreFlight,
    /// After forwarding: this request is already through, so `Block` only
    /// notifies and any refusal applies to subsequent requests.
    PostFlight,
}

/// What the caller should do with the request after a violation is handled.
#[derive(Debug)]
pub enum ViolationOutcome {
    /// Nothing to refuse (a warning, or a post-flight violation).
    Continue,
    /// Refuse with 429 and this message.
    Blocked(String),
    /// Refuse with 503 and this message; the proxy is now paused.
    Paused(String),
}

/// Post-flight bookkeeping for one forwarded request: release the in-flight
/// reservations, then evaluate time limits against the real duration.
///
/// This is a value the proxy hands off rather than code it runs inline, because
/// a streaming response resolves the forward at the *first* byte. The settlement
/// travels into the task pumping the stream and runs when the last byte is
/// through — otherwise `ConcurrentRequests` would stop counting a stream that is
/// still open and `TimeSec` would measure time-to-first-byte.
pub struct RequestSettlement {
    state: Arc<AppState>,
    start: Instant,
    provider_id: i64,
    project_tag: Option<String>,
    model: String,
    reservations: Vec<(i64, f64)>,
    group_reservations: Vec<(i64, f64)>,
    released: bool,
}

impl RequestSettlement {
    pub fn new(
        state: Arc<AppState>,
        start: Instant,
        provider_id: i64,
        project_tag: Option<String>,
        model: String,
        check: &LimitCheckResult,
    ) -> Self {
        Self {
            state,
            start,
            provider_id,
            project_tag,
            model,
            reservations: check.reservations.clone(),
            group_reservations: check.group_reservations.clone(),
            released: false,
        }
    }

    /// Release the reservations, then run the post-flight time-limit check.
    /// Call this once the request's usage has been logged.
    pub fn settle(mut self) {
        self.release();
        let duration_ms = self.start.elapsed().as_millis() as u64;
        let time_check = self.state.check_time_limits(
            self.provider_id,
            self.project_tag.as_deref(),
            Some(&self.model),
            duration_ms,
        );
        for v in &time_check.violations {
            self.state
                .apply_violation(&v.into(), LimitPhase::PostFlight);
        }
        for v in &time_check.group_violations {
            self.state
                .apply_violation(&v.into(), LimitPhase::PostFlight);
        }
    }

    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        self.state.release_request_limits(&self.reservations);
        self.state.release_group_limits(&self.group_reservations);
    }
}

impl Drop for RequestSettlement {
    /// Safety net for paths that never reach `settle` — a refused request, or a
    /// handler future cancelled because the client hung up. Without this, a
    /// `ConcurrentRequests` reservation would be held for the process lifetime.
    fn drop(&mut self) {
        if !self.released {
            tracing::debug!("request limits released without settling (refused or cancelled)");
            self.release();
        }
    }
}

pub struct AppState {
    pub db: DbPool,
    pub db_path: std::path::PathBuf,
    pub config: RwLock<Config>,
    pub paused: AtomicBool,
    pub client: reqwest::Client,
    /// Optional notifier. None in headless/CLI mode, where desktop
    /// notifications are unavailable.
    pub notifier: Option<Box<dyn Notifier + Send + Sync>>,
    /// Per-limit cooldown so warning notifications don't spam every request.
    last_warning: Mutex<HashMap<i64, Instant>>,
    /// Per-limit cooldown so block/pause notifications don't spam.
    last_block_notify: Mutex<HashMap<i64, Instant>>,
    /// Per-project cooldown so budget warning notifications don't spam.
    last_budget_warning: Mutex<HashMap<String, Instant>>,
    /// Signal the proxy server to stop accepting new connections on shutdown.
    shutdown_tx: watch::Sender<()>,
    /// In-flight request counters for atomic request-limit enforcement.
    limit_counters: LimitCounters,
    /// In-flight request counters for atomic limit-group enforcement.
    group_counters: LimitCounters,
    /// Monotonic request IDs for tracing.
    next_request_id: AtomicU64,
    /// Tracks left-clicks on the tray icon to distinguish single vs double clicks.
    #[cfg(feature = "gui")]
    tray_click: Mutex<TrayClickState>,
    /// Cached provider health check results.
    provider_health: Arc<Mutex<HealthCache>>,
}

#[derive(Default)]
#[cfg(feature = "gui")]
struct TrayClickState {
    count: u32,
    last_ms: u64,
    timer_running: bool,
}

impl AppState {
    pub fn new(
        pool: DbPool,
        db_path: std::path::PathBuf,
        config: Config,
        notifier: Option<Box<dyn Notifier + Send + Sync>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .pool_max_idle_per_host(8)
            .user_agent(format!("TokenGuard/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("reqwest client build failed: {e}"))?;
        let (shutdown_tx, _shutdown_rx) = watch::channel(());
        Ok(Self {
            db: pool,
            db_path,
            config: RwLock::new(config),
            paused: AtomicBool::new(false),
            client,
            notifier,
            last_warning: Mutex::new(HashMap::new()),
            last_block_notify: Mutex::new(HashMap::new()),
            last_budget_warning: Mutex::new(HashMap::new()),
            shutdown_tx,
            limit_counters: LimitCounters::new(),
            group_counters: LimitCounters::new(),
            next_request_id: AtomicU64::new(1),
            #[cfg(feature = "gui")]
            tray_click: Mutex::new(TrayClickState::default()),
            provider_health: Arc::new(Mutex::new(HealthCache::default())),
        })
    }

    /// Signal the proxy server to shut down gracefully.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Subscribe to the shutdown signal for the proxy server.
    pub fn shutdown_rx(&self) -> watch::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Allocate the next request ID for tracing.
    pub fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Non-mutating peek: is a block/pause notification for this limit out of
    /// cooldown? Pair with mark_block_notified() when it actually notifies.
    pub fn can_notify_block(&self, limit_id: i64) -> bool {
        cooldown_peek(&self.last_block_notify, &limit_id)
    }

    /// Record that a block/pause notification was sent for this limit.
    pub fn mark_block_notified(&self, limit_id: i64) {
        cooldown_mark(&self.last_block_notify, limit_id);
    }

    /// Non-mutating peek: is a warning notification for this limit out of
    /// cooldown? Pair with mark_warning_notified() when it actually notifies.
    pub fn can_notify_warning(&self, limit_id: i64) -> bool {
        cooldown_peek(&self.last_warning, &limit_id)
    }

    /// Record that a warning notification was sent for this limit.
    pub fn mark_warning_notified(&self, limit_id: i64) {
        cooldown_mark(&self.last_warning, limit_id);
    }

    /// Non-mutating peek: is a budget warning for this project out of
    /// cooldown? Pair with mark_budget_notified() when it actually notifies.
    pub fn can_notify_budget(&self, project_tag: &str) -> bool {
        cooldown_peek(&self.last_budget_warning, project_tag)
    }

    /// Record that a budget warning notification was sent for this project.
    pub fn mark_budget_notified(&self, project_tag: &str) {
        cooldown_mark(&self.last_budget_warning, project_tag.to_string());
    }

    /// Desktop-notification helpers. No-ops when running headlessly (no
    /// Tauri handle), so the CLI binary does not need a GUI runtime.
    pub fn notify_limit_warning(&self, name: &str, used: f64, cap: f64) {
        if let Some(n) = &self.notifier {
            n.limit_warning(name, used, cap);
        }
    }
    pub fn notify_limit_blocked(&self, name: &str, used: f64, cap: f64) {
        if let Some(n) = &self.notifier {
            n.limit_blocked(name, used, cap);
        }
    }
    pub fn notify_limit_paused(&self, name: &str, used: f64, cap: f64) {
        if let Some(n) = &self.notifier {
            n.limit_paused(name, used, cap);
        }
    }
    pub fn notify_proxy_paused(&self) {
        if let Some(n) = &self.notifier {
            n.proxy_paused();
        }
    }
    pub fn notify_proxy_resumed(&self) {
        if let Some(n) = &self.notifier {
            n.proxy_resumed();
        }
    }

    /// Route a request to a provider by (format family, model name).
    /// Falls back to the default provider for that family, then None.
    pub fn route_provider(&self, family: ProviderFormat, model: &str) -> Option<Provider> {
        let Ok(cfg) = self.config.read() else {
            return None;
        };
        route_in_list(&cfg.providers, family, model)
    }

    pub fn today_spend(&self) -> f64 {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection from pool");
            return 0.0;
        };
        db::today_spend(&conn).unwrap_or(0.0)
    }

    pub fn audit(&self, event_type: &str, details: &str) {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection for audit event");
            return;
        };
        if let Err(e) = db::insert_audit_event(&conn, event_type, details) {
            tracing::warn!("failed to insert audit event: {e}");
        }
    }

    /// Check whether a project's spend in its budget period has exceeded its
    /// budget. Returns `(used, budget, action, should_notify)` when the budget
    /// is configured and exceeded; otherwise None. `should_notify` is a
    /// peek-based cooldown decision for the Warn action (always true for
    /// Block/Pause); the caller records it via mark_budget_notified().
    pub fn check_project_budget(&self, project_tag: &str) -> Option<(f64, f64, LimitAction, bool)> {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection from pool for project budget");
            return None;
        };
        let project = self
            .config
            .read()
            .ok()?
            .projects
            .iter()
            .find(|p| p.name == project_tag)?
            .clone();
        if project.budget <= 0.0 {
            return None;
        }
        let used =
            db::project_period_spend(&conn, project_tag, project.budget_period).unwrap_or(0.0);
        if used >= project.budget {
            let should_notify = match project.budget_action {
                LimitAction::Warn => self.can_notify_budget(project_tag),
                _ => true,
            };
            Some((used, project.budget, project.budget_action, should_notify))
        } else {
            None
        }
    }

    /// Map a client-supplied API key (the label_key the user set in their
    /// coding agent) to a project name. None if no project matches.
    pub fn project_for_key(&self, key: &str) -> Option<String> {
        let Ok(cfg) = self.config.read() else {
            return None;
        };
        cfg.projects
            .iter()
            .find(|p| p.label_key == key)
            .map(|p| p.name.clone())
    }

    /// Check enabled limits and return those currently exceeded.
    /// Also emits a desktop warning notification when a limit crosses its
    /// warning threshold (but hasn't hit the cap yet), throttled per limit.
    ///
    /// Request limits are enforced atomically via in-memory counters so a burst
    /// of concurrent requests cannot overshoot the cap between the check and
    /// the log insert. Each check reserves +1 per request-based limit; the
    /// reservation is released exactly once by the caller (see
    /// release_request_limits) — check_limits never decrements internally.
    ///
    /// `duration_ms` is only meaningful for `TimeSec` limits. For the pre-flight
    /// check it should be `0`; use `check_time_limits` after the request to
    /// evaluate time limits against the real duration.
    pub fn check_limits(
        &self,
        provider_id: i64,
        project_tag: Option<&str>,
        model: Option<&str>,
        cost: f64,
        tokens: u64,
        duration_ms: u64,
    ) -> LimitCheckResult {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection from pool for check_limits");
            return LimitCheckResult::default();
        };
        // Clone what we need and drop the read guard before any DB work.
        let (limits, groups, projects, webhook_url) = match self.config.read() {
            Ok(cfg) => (
                cfg.limits.clone(),
                cfg.limit_groups.clone(),
                cfg.projects.clone(),
                cfg.webhook_url.clone(),
            ),
            Err(_) => return LimitCheckResult::default(),
        };

        let mut result = LimitCheckResult::default();
        for limit in &limits {
            if !limit.enabled {
                continue;
            }
            if !limit_scope_matches(limit, provider_id, project_tag, model, &projects) {
                continue;
            }
            if !is_limit_active(limit) {
                continue;
            }

            let persisted = db::usage_for_limit(&conn, limit).unwrap_or(0.0);

            // For request-based limits and tokens-per-minute, reserve the expected
            // amount in the atomic counter first so concurrent requests see each
            // other. The counter is keyed by limit id only and holds just in-flight
            // reservations; the persisted DB count handles the time window. The
            // caller releases the reservation exactly once per terminal outcome.
            let (current, used) = if limit.metric == LimitMetric::Requests
                || limit.metric == LimitMetric::RequestsPerMinute
            {
                let reserved = self.limit_counters.increment(limit.id, 1.0);
                result.reservations.push((limit.id, 1.0));
                (1.0, persisted + reserved - 1.0)
            } else if limit.metric == LimitMetric::TokensPerMinute {
                let reserved = self.limit_counters.increment(limit.id, tokens as f64);
                result.reservations.push((limit.id, tokens as f64));
                (tokens as f64, persisted + reserved - tokens as f64)
            } else if limit.metric == LimitMetric::ConcurrentRequests {
                // In-flight concurrency: the reservation lives for the whole
                // request and is released when the proxy finishes forwarding.
                let reserved = self.limit_counters.increment(limit.id, 1.0);
                result.reservations.push((limit.id, 1.0));
                (1.0, reserved - 1.0)
            } else {
                let current = match limit.metric {
                    LimitMetric::Money => cost,
                    LimitMetric::Tokens => tokens as f64,
                    // Prompt size is unknown until the response arrives; the
                    // persisted sum covers the period, the current request is
                    // counted by the DB after forwarding.
                    LimitMetric::InputTokens => 0.0,
                    LimitMetric::OutputTokens => tokens as f64,
                    // Per-request cost cap compares the estimated cost of this
                    // single request against the cap; there is no history.
                    LimitMetric::CostPerRequest => cost,
                    LimitMetric::TimeSec => duration_ms as f64 / 1000.0,
                    LimitMetric::Requests
                    | LimitMetric::RequestsPerMinute
                    | LimitMetric::TokensPerMinute
                    | LimitMetric::ConcurrentRequests => unreachable!(),
                };
                (current, persisted)
            };

            let total = used + current;
            if limit.cap > 0.0 && total >= limit.cap {
                audit_with_conn(
                    &conn,
                    "limit_hit",
                    &format!(
                        "limit={} metric={} action={} used={:.4} cap={:.4}",
                        limit.name,
                        limit.metric.as_db_str(),
                        limit.action.as_db_str(),
                        total,
                        limit.cap
                    ),
                );
                // Peek only; the proxy records the cooldown when it notifies.
                let should_notify = match limit.action {
                    LimitAction::Warn => self.can_notify_warning(limit.id),
                    _ => self.can_notify_block(limit.id),
                };
                if should_notify {
                    if let Some(ref url) = webhook_url {
                        webhook::send_limit_event(
                            &self.client,
                            url,
                            "limit_hit",
                            limit,
                            total,
                            limit.cap,
                        );
                    }
                }
                result.violations.push(LimitViolation {
                    limit: limit.clone(),
                    used,
                    should_notify,
                });
                continue;
            }

            // Warning threshold notification (throttled). This path notifies
            // directly, so it also records the cooldown here.
            if limit.cap > 0.0
                && limit.warning_threshold > 0.0
                && used >= limit.warning_threshold * limit.cap
                && self.can_notify_warning(limit.id)
            {
                audit_with_conn(
                    &conn,
                    "limit_warning",
                    &format!(
                        "limit={} metric={} used={:.4} cap={:.4} threshold={:.2}",
                        limit.name,
                        limit.metric.as_db_str(),
                        used,
                        limit.cap,
                        limit.warning_threshold
                    ),
                );
                self.notify_limit_warning(&limit.name, used, limit.cap);
                if let Some(ref url) = webhook_url {
                    webhook::send_limit_event(
                        &self.client,
                        url,
                        "limit_warning",
                        limit,
                        used,
                        limit.cap,
                    );
                }
                self.mark_warning_notified(limit.id);
            }
        }

        // Check limit groups using the same semantics as individual limits.
        for group in &groups {
            if !group.enabled {
                continue;
            }
            if !limit_scope_matches_group(group, provider_id, project_tag, model, &projects) {
                continue;
            }
            if !is_schedule_active(
                group.active_days,
                group.active_hours_start.as_deref(),
                group.active_hours_end.as_deref(),
            ) {
                continue;
            }

            let persisted = db::usage_for_limit_group(&conn, group).unwrap_or(0.0);

            let (current, used) = if group.metric == LimitMetric::Requests
                || group.metric == LimitMetric::RequestsPerMinute
            {
                let reserved = self.group_counters.increment(group.id, 1.0);
                result.group_reservations.push((group.id, 1.0));
                (1.0, persisted + reserved - 1.0)
            } else if group.metric == LimitMetric::TokensPerMinute {
                let reserved = self.group_counters.increment(group.id, tokens as f64);
                result.group_reservations.push((group.id, tokens as f64));
                (tokens as f64, persisted + reserved - tokens as f64)
            } else if group.metric == LimitMetric::ConcurrentRequests {
                let reserved = self.group_counters.increment(group.id, 1.0);
                result.group_reservations.push((group.id, 1.0));
                (1.0, reserved - 1.0)
            } else {
                let current = match group.metric {
                    LimitMetric::Money => cost,
                    LimitMetric::Tokens => tokens as f64,
                    LimitMetric::InputTokens => 0.0,
                    LimitMetric::OutputTokens => tokens as f64,
                    LimitMetric::CostPerRequest => cost,
                    LimitMetric::TimeSec => duration_ms as f64 / 1000.0,
                    LimitMetric::Requests
                    | LimitMetric::RequestsPerMinute
                    | LimitMetric::TokensPerMinute
                    | LimitMetric::ConcurrentRequests => unreachable!(),
                };
                (current, persisted)
            };

            let total = used + current;
            if group.cap > 0.0 && total >= group.cap {
                audit_with_conn(
                    &conn,
                    "limit_group_hit",
                    &format!(
                        "group={} metric={} action={} used={:.4} cap={:.4}",
                        group.name,
                        group.metric.as_db_str(),
                        group.action.as_db_str(),
                        total,
                        group.cap
                    ),
                );
                let should_notify = match group.action {
                    LimitAction::Warn => self.can_notify_warning(group.id),
                    _ => self.can_notify_block(group.id),
                };
                if should_notify {
                    if let Some(ref url) = webhook_url {
                        webhook::send_limit_group_event(
                            &self.client,
                            url,
                            "limit_group_hit",
                            group,
                            total,
                            group.cap,
                        );
                    }
                }
                result.group_violations.push(GroupViolation {
                    group: group.clone(),
                    used,
                    should_notify,
                });
                continue;
            }

            if group.cap > 0.0
                && group.warning_threshold > 0.0
                && used >= group.warning_threshold * group.cap
                && self.can_notify_warning(group.id)
            {
                audit_with_conn(
                    &conn,
                    "limit_group_warning",
                    &format!(
                        "group={} metric={} used={:.4} cap={:.4} threshold={:.2}",
                        group.name,
                        group.metric.as_db_str(),
                        used,
                        group.cap,
                        group.warning_threshold
                    ),
                );
                self.notify_limit_warning(&group.name, used, group.cap);
                if let Some(ref url) = webhook_url {
                    webhook::send_limit_group_event(
                        &self.client,
                        url,
                        "limit_group_warning",
                        group,
                        used,
                        group.cap,
                    );
                }
                self.mark_warning_notified(group.id);
            }
        }

        result
    }

    /// Post-flight check for time-based limits only. Time limits cannot be
    /// estimated before the upstream request completes, so they are evaluated
    /// after the request has finished and its real duration is known. Returns
    /// any TimeSec limit that is now exceeded so the proxy can warn/pause.
    pub fn check_time_limits(
        &self,
        provider_id: i64,
        project_tag: Option<&str>,
        model: Option<&str>,
        duration_ms: u64,
    ) -> LimitCheckResult {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection from pool for check_time_limits");
            return LimitCheckResult::default();
        };
        let (limits, groups, projects, webhook_url) = match self.config.read() {
            Ok(cfg) => (
                cfg.limits.clone(),
                cfg.limit_groups.clone(),
                cfg.projects.clone(),
                cfg.webhook_url.clone(),
            ),
            Err(_) => return LimitCheckResult::default(),
        };

        let mut result = LimitCheckResult::default();
        for limit in &limits {
            if !limit.enabled || limit.metric != LimitMetric::TimeSec {
                continue;
            }
            if !limit_scope_matches(limit, provider_id, project_tag, model, &projects) {
                continue;
            }
            if !is_limit_active(limit) {
                continue;
            }

            let persisted = db::usage_for_limit(&conn, limit).unwrap_or(0.0);
            let current = duration_ms as f64 / 1000.0;
            let total = persisted + current;
            if limit.cap > 0.0 && total >= limit.cap {
                audit_with_conn(
                    &conn,
                    "limit_hit",
                    &format!(
                        "limit={} metric={} action={} used={:.4} cap={:.4}",
                        limit.name,
                        limit.metric.as_db_str(),
                        limit.action.as_db_str(),
                        total,
                        limit.cap
                    ),
                );
                let should_notify = match limit.action {
                    LimitAction::Warn => self.can_notify_warning(limit.id),
                    _ => self.can_notify_block(limit.id),
                };
                if should_notify {
                    if let Some(ref url) = webhook_url {
                        webhook::send_limit_event(
                            &self.client,
                            url,
                            "limit_hit",
                            limit,
                            total,
                            limit.cap,
                        );
                    }
                }
                result.violations.push(LimitViolation {
                    limit: limit.clone(),
                    used: persisted,
                    should_notify,
                });
                continue;
            }

            if limit.cap > 0.0
                && limit.warning_threshold > 0.0
                && persisted >= limit.warning_threshold * limit.cap
                && self.can_notify_warning(limit.id)
            {
                audit_with_conn(
                    &conn,
                    "limit_warning",
                    &format!(
                        "limit={} metric={} used={:.4} cap={:.4} threshold={:.2}",
                        limit.name,
                        limit.metric.as_db_str(),
                        persisted,
                        limit.cap,
                        limit.warning_threshold
                    ),
                );
                self.notify_limit_warning(&limit.name, persisted, limit.cap);
                if let Some(ref url) = webhook_url {
                    webhook::send_limit_event(
                        &self.client,
                        url,
                        "limit_warning",
                        limit,
                        persisted,
                        limit.cap,
                    );
                }
                self.mark_warning_notified(limit.id);
            }
        }

        // Post-flight time check for limit groups.
        for group in &groups {
            if !group.enabled || group.metric != LimitMetric::TimeSec {
                continue;
            }
            if !limit_scope_matches_group(group, provider_id, project_tag, model, &projects) {
                continue;
            }
            if !is_schedule_active(
                group.active_days,
                group.active_hours_start.as_deref(),
                group.active_hours_end.as_deref(),
            ) {
                continue;
            }

            let persisted = db::usage_for_limit_group(&conn, group).unwrap_or(0.0);
            let current = duration_ms as f64 / 1000.0;
            let total = persisted + current;
            if group.cap > 0.0 && total >= group.cap {
                audit_with_conn(
                    &conn,
                    "limit_group_hit",
                    &format!(
                        "group={} metric={} action={} used={:.4} cap={:.4}",
                        group.name,
                        group.metric.as_db_str(),
                        group.action.as_db_str(),
                        total,
                        group.cap
                    ),
                );
                let should_notify = match group.action {
                    LimitAction::Warn => self.can_notify_warning(group.id),
                    _ => self.can_notify_block(group.id),
                };
                if should_notify {
                    if let Some(ref url) = webhook_url {
                        webhook::send_limit_group_event(
                            &self.client,
                            url,
                            "limit_group_hit",
                            group,
                            total,
                            group.cap,
                        );
                    }
                }
                result.group_violations.push(GroupViolation {
                    group: group.clone(),
                    used: persisted,
                    should_notify,
                });
                continue;
            }

            if group.cap > 0.0
                && group.warning_threshold > 0.0
                && persisted >= group.warning_threshold * group.cap
                && self.can_notify_warning(group.id)
            {
                audit_with_conn(
                    &conn,
                    "limit_group_warning",
                    &format!(
                        "group={} metric={} used={:.4} cap={:.4} threshold={:.2}",
                        group.name,
                        group.metric.as_db_str(),
                        persisted,
                        group.cap,
                        group.warning_threshold
                    ),
                );
                self.notify_limit_warning(&group.name, persisted, group.cap);
                if let Some(ref url) = webhook_url {
                    webhook::send_limit_group_event(
                        &self.client,
                        url,
                        "limit_group_warning",
                        group,
                        persisted,
                        group.cap,
                    );
                }
                self.mark_warning_notified(group.id);
            }
        }

        result
    }

    /// Release in-flight reservations made by check_limits. Each reservation
    /// must be released exactly once per terminal outcome: immediately when
    /// the request is blocked/paused, or after the request is logged on
    /// success (the DB count then includes it, so the reservation is
    /// redundant). Releasing clamps at zero, never going negative.
    pub fn release_request_limits(&self, reservations: &[(i64, f64)]) {
        for (id, amount) in reservations {
            self.limit_counters.release(*id, *amount);
        }
    }

    pub fn release_group_limits(&self, reservations: &[(i64, f64)]) {
        for (id, amount) in reservations {
            self.group_counters.release(*id, *amount);
        }
    }

    /// Notify, pause and audit for one violation, and report whether the
    /// request should be refused. Identical for limits and limit groups, and for
    /// pre- and post-flight checks apart from whether refusal is still possible.
    pub fn apply_violation(&self, v: &ViolationAction, phase: LimitPhase) -> ViolationOutcome {
        let refusable = phase == LimitPhase::PreFlight;
        match v.action {
            LimitAction::Block => {
                if v.should_notify {
                    self.notify_limit_blocked(&v.name, v.used, v.cap);
                    self.mark_block_notified(v.id);
                }
                let msg = format!(
                    "{} exceeded: {} ({:.0}/{:.0})",
                    v.kind, v.name, v.used, v.cap
                );
                if refusable {
                    ViolationOutcome::Blocked(msg)
                } else {
                    tracing::warn!("{msg} (request already forwarded)");
                    ViolationOutcome::Continue
                }
            }
            LimitAction::Pause => {
                if v.should_notify {
                    self.notify_limit_paused(&v.name, v.used, v.cap);
                    self.mark_block_notified(v.id);
                }
                self.set_paused(true);
                let msg = format!("{} exceeded: {} — proxy paused", v.kind, v.name);
                if refusable {
                    ViolationOutcome::Paused(msg)
                } else {
                    tracing::warn!("{msg} (request already forwarded)");
                    ViolationOutcome::Continue
                }
            }
            LimitAction::Warn => {
                if v.should_notify {
                    self.notify_limit_warning(&v.name, v.used, v.cap);
                    self.mark_warning_notified(v.id);
                }
                tracing::warn!(
                    "{} warning: {} ({:.0}/{:.0})",
                    v.kind,
                    v.name,
                    v.used,
                    v.cap
                );
                ViolationOutcome::Continue
            }
        }
    }

    /// In-flight reservation total for one limit id. Test-only observability.
    #[cfg(test)]
    pub fn in_flight_for_limit(&self, limit_id: i64) -> f64 {
        self.limit_counters.get(limit_id)
    }

    /// Reserve directly in the in-flight counter, standing in for what
    /// `check_limits` would have reserved. Test-only.
    #[cfg(test)]
    pub fn check_limits_test_reserve(&self, limit_id: i64, amount: f64) {
        self.limit_counters.increment(limit_id, amount);
    }

    /// Compute the most critical active limit status for the tray.
    /// Returns (overall_ratio, critical_limit_name_or_none).
    pub fn limit_status(&self) -> (f64, Option<String>) {
        let Ok(conn) = self.db.get() else {
            return (0.0, None);
        };
        let Ok(cfg) = self.config.read() else {
            return (0.0, None);
        };

        let mut max_ratio = 0.0;
        let mut critical: Option<String> = None;
        for limit in &cfg.limits {
            if !limit.enabled || limit.cap <= 0.0 {
                continue;
            }
            let used = db::usage_for_limit(&conn, limit).unwrap_or(0.0);
            let ratio = used / limit.cap;
            if ratio > max_ratio {
                max_ratio = ratio;
                critical = Some(limit.name.clone());
            }
        }
        for group in &cfg.limit_groups {
            if !group.enabled || group.cap <= 0.0 {
                continue;
            }
            let used = db::usage_for_limit_group(&conn, group).unwrap_or(0.0);
            let ratio = used / group.cap;
            if ratio > max_ratio {
                max_ratio = ratio;
                critical = Some(format!("{} (group)", group.name));
            }
        }
        (max_ratio, critical)
    }

    /// Insert a request log from a background thread, then refresh the tray.
    #[allow(clippy::too_many_arguments)]
    pub async fn log_request(
        self: Arc<Self>,
        provider: Provider,
        model: String,
        prompt: u64,
        completion: u64,
        cost: f64,
        duration_ms: u64,
        project_tag: Option<String>,
        status: Option<u16>,
    ) {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let Ok(conn) = this.db.get() else {
                tracing::error!("failed to get DB connection from pool for log_request");
                this.refresh_tray();
                return;
            };
            if let Err(e) = db::insert_log(
                &conn,
                &provider.name,
                &model,
                prompt,
                completion,
                cost,
                duration_ms,
                project_tag.as_deref(),
                status,
            ) {
                tracing::error!("insert_log failed: {e}");
            }
            this.refresh_tray();
        })
        .await
        .ok();
    }

    /// Rebuild the tray menu + tooltip + icon color from current state.
    /// No-op in headless mode.
    pub fn refresh_tray(&self) {
        #[cfg(feature = "gui")]
        {
            let spend = self.today_spend();
            let (ratio, critical) = self.limit_status();
            let paused = self.paused.load(Ordering::Relaxed);

            let icon_bytes = if paused {
                ICON_ORANGE
            } else if ratio >= 1.0 {
                ICON_RED
            } else if ratio >= 0.8 {
                ICON_YELLOW
            } else {
                ICON_GREEN
            };

            let budget = self.config.read().map(|cfg| cfg.budget).unwrap_or(0.0);
            let critical_deref = critical.as_deref();
            let tooltip = format!(
                "Token Guard — ${spend:.2} today{paused}{critical}",
                paused = if paused { " (paused)" } else { "" },
                critical = critical_deref
                    .map(|c| format!(" — limit: {c}"))
                    .unwrap_or_default(),
            );
            // The notifier is only present in GUI mode, but tray access still
            // requires the original Tauri handle. Retrieve it from the notifier
            // via the gui-only accessor.
            if let Some(handle) = self.tauri_handle() {
                if let Some(tray) = handle.tray_by_id("main") {
                    let _ = tray.set_tooltip(Some(&tooltip));
                    if let Ok(img) = tauri::image::Image::from_bytes(icon_bytes) {
                        let _ = tray.set_icon(Some(img));
                    }
                    if let Ok(menu) =
                        build_tray_menu(&handle, spend, budget, paused, critical_deref)
                    {
                        let _ = tray.set_menu(Some(menu));
                    }
                }
            }
        }
    }

    /// GUI-only accessor for the Tauri handle stored inside the notifier.
    #[cfg(feature = "gui")]
    fn tauri_handle(&self) -> Option<AppHandle<Wry>> {
        use crate::notifications::TauriNotifier;
        use std::any::Any;
        self.notifier
            .as_ref()
            .and_then(|n| (n.as_ref() as &dyn Any).downcast_ref::<TauriNotifier>())
            .map(|n| n.handle())
    }

    pub fn all_provider_health(&self) -> std::collections::HashMap<i64, ProviderHealth> {
        self.provider_health
            .lock()
            .ok()
            .map(|c| c.all())
            .unwrap_or_default()
    }

    pub fn provider_health_cache(&self) -> Arc<Mutex<HealthCache>> {
        self.provider_health.clone()
    }

    /// Idempotently set the paused flag. Budget/limit enforcement uses this
    /// (instead of toggling) so concurrent violations can't flip the proxy
    /// back to unpaused. Notifications and tray refresh fire only on change.
    pub fn set_paused(&self, paused: bool) {
        let was = self.paused.swap(paused, Ordering::Relaxed);
        if was == paused {
            return;
        }
        if paused {
            self.notify_proxy_paused();
        } else {
            self.notify_proxy_resumed();
        }
        self.refresh_tray();
    }

    /// User-facing tray action: flip the paused flag. Returns the new state.
    pub fn toggle_pause(&self) -> bool {
        let new = !self.paused.load(Ordering::Relaxed);
        self.set_paused(new);
        new
    }
}

/// Insert an audit event on an already-held connection, so the proxy hot path
/// doesn't take a second pooled connection per event.
fn audit_with_conn(conn: &rusqlite::Connection, event_type: &str, details: &str) {
    if let Err(e) = db::insert_audit_event(conn, event_type, details) {
        tracing::warn!("failed to insert audit event: {e}");
    }
}

/// True when `key` has no recorded notification within the cooldown window.
/// Does not record anything; use cooldown_mark() after actually notifying.
fn cooldown_peek<K, Q>(map: &Mutex<HashMap<K, Instant>>, key: &Q) -> bool
where
    K: Eq + std::hash::Hash + std::borrow::Borrow<Q>,
    Q: Eq + std::hash::Hash + ?Sized,
{
    let now = Instant::now();
    map.lock()
        .map(|m| cooldown_allows(m.get(key).copied(), now))
        .unwrap_or(true)
}

fn cooldown_mark<K: Eq + std::hash::Hash>(map: &Mutex<HashMap<K, Instant>>, key: K) {
    if let Ok(mut m) = map.lock() {
        m.insert(key, Instant::now());
    }
}

fn cooldown_allows(last: Option<Instant>, now: Instant) -> bool {
    last.map(|l| now.duration_since(l) >= WARNING_COOLDOWN)
        .unwrap_or(true)
}

/// Parse a "HH:MM" string into minutes since midnight.
fn parse_minutes(s: &str) -> Option<u32> {
    let mut parts = s.split(':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    if h >= 24 || m >= 60 {
        return None;
    }
    Some(h * 60 + m)
}

/// Return true if the current UTC time falls inside the limit's optional
/// schedule. A missing schedule means always active.
pub fn is_limit_active(limit: &Limit) -> bool {
    is_schedule_active(
        limit.active_days,
        limit.active_hours_start.as_deref(),
        limit.active_hours_end.as_deref(),
    )
}

fn is_schedule_active(
    active_days: u8,
    active_hours_start: Option<&str>,
    active_hours_end: Option<&str>,
) -> bool {
    let now = chrono::Utc::now();

    if active_days != 0b1111111 {
        // Bit 0 = Monday .. bit 6 = Sunday.
        let weekday = now.weekday().num_days_from_monday() as u8;
        if active_days & (1 << weekday) == 0 {
            return false;
        }
    }

    if let (Some(start), Some(end)) = (active_hours_start, active_hours_end) {
        if let (Some(start_min), Some(end_min)) = (parse_minutes(start), parse_minutes(end)) {
            let cur = now.hour() * 60 + now.minute();
            if start_min <= end_min {
                return cur >= start_min && cur <= end_min;
            } else {
                return cur >= start_min || cur <= end_min;
            }
        }
    }

    true
}

#[cfg(feature = "gui")]
fn build_tray_menu(
    app: &AppHandle<Wry>,
    spend: f64,
    budget: f64,
    paused: bool,
    critical: Option<&str>,
) -> Result<Menu<Wry>, tauri::Error> {
    let spend_item = MenuItem::with_id(
        app,
        "spend",
        format!("Today's spend: ${spend:.2}"),
        false,
        None::<&str>,
    )?;
    let budget_text = if budget > 0.0 {
        format!("Budget: ${spend:.2} / ${budget:.2}")
    } else {
        "Budget: not set".to_string()
    };
    let budget_item = MenuItem::with_id(app, "budget", budget_text, false, None::<&str>)?;
    let status_item = MenuItem::with_id(
        app,
        "status",
        if paused {
            "Status: paused".to_string()
        } else {
            "Status: active".to_string()
        },
        false,
        None::<&str>,
    )?;
    let critical_text = critical.map(|c| format!("Limit: {c}"));
    let critical_item = critical_text
        .as_ref()
        .map(|c| MenuItem::with_id(app, "critical", c.clone(), false, None::<&str>))
        .transpose()?;
    let open_item = MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let pause_item = MenuItem::with_id(
        app,
        "pause",
        if paused {
            "Resume proxy"
        } else {
            "Pause proxy"
        },
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        vec![&spend_item, &budget_item, &status_item];
    if let Some(ref c) = critical_item {
        items.push(c);
    }
    items.push(&sep1);
    items.push(&open_item);
    items.push(&settings_item);
    items.push(&sep2);
    items.push(&pause_item);
    items.push(&sep3);
    items.push(&quit_item);
    Menu::with_items(app, &items)
}

/// Build the tray icon at startup. Left-click toggles pause.
#[cfg(feature = "gui")]
pub fn build_tray(app: &AppHandle<Wry>) -> Result<(), tauri::Error> {
    let menu = build_tray_menu(app, 0.0, 0.0, false, None)?;
    let icon = tauri::image::Image::from_bytes(ICON_GREEN)?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Token Guard")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                handle_tray_left_click(app);
            }
        })
        .build(app)?;
    Ok(())
}

/// Show the main window and tell the UI which tab to activate.
#[cfg(feature = "gui")]
fn show_tab(app: &AppHandle<Wry>, tab: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
    let _ = app.emit("set_tab", tab);
}

#[cfg(feature = "gui")]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Single left-click pauses/resumes; two quick left-clicks open the dashboard.
#[cfg(feature = "gui")]
fn handle_tray_left_click(app: &AppHandle<Wry>) {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return;
    };
    let t = now_ms();
    let needs_timer = {
        let mut tray = state.tray_click.lock().unwrap();
        tray.count += 1;
        tray.last_ms = t;
        let needs = !tray.timer_running;
        tray.timer_running = needs;
        needs
    };

    if !needs_timer {
        return;
    }

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let (stable, count) = {
                let state = app2.state::<Arc<AppState>>();
                let tray = state.tray_click.lock().unwrap();
                let elapsed = now_ms().saturating_sub(tray.last_ms);
                (elapsed > 250, tray.count)
            };
            if stable {
                let state = app2.state::<Arc<AppState>>();
                let mut tray = state.tray_click.lock().unwrap();
                tray.timer_running = false;
                tray.count = 0;
                drop(tray);
                if count == 1 {
                    state.toggle_pause();
                } else {
                    show_tab(&app2, "dashboard");
                }
                break;
            }
        }
    });
}

/// Menu item click handler (registered in lib.rs).
#[cfg(feature = "gui")]
pub fn handle_menu_event(app: &AppHandle<Wry>, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "open" => show_tab(app, "dashboard"),
        "settings" => show_tab(app, "settings"),
        "pause" => {
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                state.inner().toggle_pause();
            }
        }
        "quit" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::graceful_exit(&app).await;
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthScheme, ProviderFormat};

    fn provider(name: &str, family: ProviderFormat, models: &[&str], is_default: bool) -> Provider {
        Provider {
            id: 0,
            name: name.to_string(),
            base_url: "https://example.com".to_string(),
            format: family,
            auth: AuthScheme::Bearer,
            models: models
                .iter()
                .map(|s| crate::config::ModelMapping {
                    local: s.to_string(),
                    remote: s.to_string(),
                    pricing: crate::cost::PricingProfile::default(),
                })
                .collect(),
            is_default,
            fallback_provider_id: None,
            extra_headers: Vec::new(),
        }
    }

    #[test]
    fn route_by_model_match() {
        let providers = vec![
            provider("OpenAI", ProviderFormat::OpenAI, &["gpt-4o"], true),
            provider("Anthropic", ProviderFormat::Anthropic, &["claude-3"], true),
        ];
        let chosen = route_in_list(&providers, ProviderFormat::OpenAI, "gpt-4o");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    #[test]
    fn route_falls_back_to_default() {
        let providers = vec![
            provider("OpenAI", ProviderFormat::OpenAI, &["gpt-4o"], true),
            provider("Backup", ProviderFormat::OpenAI, &["gpt-3.5"], false),
        ];
        let chosen = route_in_list(&providers, ProviderFormat::OpenAI, "unknown-model");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    #[test]
    fn route_falls_back_to_any_provider() {
        let providers = vec![provider(
            "OpenAI",
            ProviderFormat::OpenAI,
            &["gpt-4o"],
            false,
        )];
        let chosen = route_in_list(&providers, ProviderFormat::Anthropic, "other");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    #[test]
    fn route_matches_across_formats() {
        let providers = vec![provider(
            "OpenAI",
            ProviderFormat::OpenAI,
            &["gpt-4o"],
            false,
        )];
        let chosen = route_in_list(&providers, ProviderFormat::Anthropic, "gpt-4o");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    #[test]
    fn route_falls_back_to_default_when_model_missing() {
        let providers = vec![provider(
            "OpenAI",
            ProviderFormat::OpenAI,
            &["gpt-4o"],
            true,
        )];
        let chosen = route_in_list(&providers, ProviderFormat::Anthropic, "claude-3");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    fn mk_limit(scope: LimitScope, scope_id: Option<i64>) -> Limit {
        Limit {
            id: 1,
            name: "test".into(),
            metric: LimitMetric::Requests,
            period: crate::config::LimitPeriod::Daily,
            cap: 10.0,
            warning_threshold: 0.8,
            scope,
            scope_id,
            action: crate::config::LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
            model_pattern: None,
        }
    }

    #[test]
    fn limit_global_scope_matches_anything() {
        let limit = mk_limit(LimitScope::Global, None);
        assert!(limit_scope_matches(&limit, 99, Some("any"), Some("m"), &[]));
    }

    #[test]
    fn limit_provider_scope_matches_id() {
        let limit = mk_limit(LimitScope::Provider, Some(7));
        assert!(limit_scope_matches(&limit, 7, None, Some("m"), &[]));
        assert!(!limit_scope_matches(&limit, 8, None, Some("m"), &[]));
    }

    #[test]
    fn limit_project_scope_matches_tag() {
        let projects = vec![Project {
            id: 3,
            name: "cursor-app".into(),
            label_key: "tg_abc".into(),
            budget: 0.0,
            budget_period: crate::config::BudgetPeriod::Daily,
            budget_action: crate::config::LimitAction::Warn,
        }];
        let limit = mk_limit(LimitScope::Project, Some(3));
        assert!(limit_scope_matches(
            &limit,
            1,
            Some("cursor-app"),
            Some("m"),
            &projects
        ));
        assert!(!limit_scope_matches(
            &limit,
            1,
            Some("other"),
            Some("m"),
            &projects
        ));
        assert!(!limit_scope_matches(&limit, 1, None, Some("m"), &projects));
    }

    #[test]
    fn limit_model_scope_matches_pattern() {
        let mut limit = mk_limit(LimitScope::Model, None);
        limit.model_pattern = Some("gpt-4".into());
        assert!(limit_scope_matches(&limit, 1, None, Some("gpt-4o"), &[]));
        assert!(!limit_scope_matches(&limit, 1, None, Some("claude"), &[]));
        assert!(!limit_scope_matches(&limit, 1, None, None, &[]));
    }

    #[test]
    fn limit_model_pattern_narrows_other_scopes() {
        let mut limit = mk_limit(LimitScope::Global, None);
        limit.model_pattern = Some("claude".into());
        assert!(limit_scope_matches(
            &limit,
            1,
            None,
            Some("claude-sonnet"),
            &[]
        ));
        assert!(!limit_scope_matches(&limit, 1, None, Some("gpt-4o"), &[]));
    }

    #[test]
    fn limit_model_scope_without_pattern_matches_everything() {
        // Rows written before add_limit validated the pattern would otherwise
        // enforce nothing at all, silently.
        let limit = mk_limit(LimitScope::Model, None);
        assert!(limit_scope_matches(&limit, 1, None, Some("gpt-4o"), &[]));
        assert!(limit_scope_matches(&limit, 1, None, None, &[]));
    }

    #[test]
    fn limit_blank_model_pattern_is_ignored() {
        // An empty text field arrives as Some("") from the UI.
        let mut limit = mk_limit(LimitScope::Global, None);
        limit.model_pattern = Some("   ".into());
        assert!(limit_scope_matches(&limit, 1, None, Some("gpt-4o"), &[]));
    }

    #[test]
    fn group_model_scope_without_pattern_matches_everything() {
        let group = LimitGroup {
            id: 1,
            name: "g".into(),
            metric: LimitMetric::Requests,
            period: crate::config::LimitPeriod::Daily,
            cap: 10.0,
            warning_threshold: 0.8,
            scope: LimitScope::Model,
            scope_id: None,
            action: crate::config::LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
            model_pattern: None,
            member_limit_ids: Vec::new(),
        };
        assert!(limit_scope_matches_group(
            &group,
            1,
            None,
            Some("gpt-4o"),
            &[]
        ));
    }

    #[test]
    fn cooldown_allows_first_notification() {
        assert!(cooldown_allows(None, Instant::now()));
    }

    #[test]
    fn cooldown_blocks_within_window() {
        let now = Instant::now();
        assert!(!cooldown_allows(Some(now), now));
        assert!(!cooldown_allows(
            Some(now),
            now + WARNING_COOLDOWN - Duration::from_secs(1)
        ));
    }

    #[test]
    fn cooldown_allows_after_window() {
        let now = Instant::now();
        assert!(cooldown_allows(
            Some(now),
            now + WARNING_COOLDOWN + Duration::from_secs(1)
        ));
    }

    #[test]
    fn cooldown_peek_does_not_record() {
        // The peek/mark split: peeking must stay true until someone records.
        let map: Mutex<HashMap<i64, Instant>> = Mutex::new(HashMap::new());
        assert!(cooldown_peek(&map, &1));
        assert!(cooldown_peek(&map, &1));
        cooldown_mark(&map, 1);
        assert!(!cooldown_peek(&map, &1));
        assert!(cooldown_peek(&map, &2));
    }
}
