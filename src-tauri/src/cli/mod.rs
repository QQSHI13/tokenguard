//! Token Guard CLI — admin tool and headless gateway.

use crate::backend;
use crate::notifier::Notifier;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

mod backup;
mod health;
mod license;
mod limits;
mod logs;
mod models;
mod projects;
mod providers;
mod proxy;
mod secrets;
mod settings;
mod update;
mod usage;

#[derive(Parser, Debug)]
#[command(
    name = "tokenguard",
    about = "Token Guard — local LLM gateway and cost tracker",
    version
)]
pub struct Cli {
    /// Data directory for the SQLite database and exports.
    #[arg(short, long, env = "TOKENGUARD_DATA_DIR", global = true)]
    data_dir: Option<PathBuf>,

    /// Increase logging verbosity.
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Start the local proxy gateway (default if no command is given).
    Start {
        /// Override the port stored in the database.
        #[arg(short, long, env = "TOKENGUARD_PORT")]
        port: Option<u16>,
        /// Expose the proxy to the LAN (0.0.0.0) instead of loopback only.
        #[arg(long, env = "TOKENGUARD_EXPOSE_TO_LAN")]
        expose_to_lan: bool,
    },
    /// Show a short status summary.
    Status,
    /// Pause, resume, or toggle the proxy.
    #[command(subcommand)]
    Proxy(ProxyCommands),
    /// Manage providers.
    #[command(subcommand)]
    Provider(ProviderCommands),
    /// Manage projects.
    #[command(subcommand)]
    Project(ProjectCommands),
    /// Manage limits.
    #[command(subcommand)]
    Limit(LimitCommands),
    /// View or change settings.
    #[command(subcommand)]
    Settings(SettingsCommands),
    /// License activation.
    #[command(subcommand)]
    License(LicenseCommands),
    /// Check for updates.
    Update {
        #[command(subcommand)]
        command: Option<UpdateCommands>,
    },
    /// Backup or restore the database.
    #[command(subcommand)]
    Backup(BackupCommands),
    /// Export, query, or audit logs.
    #[command(subcommand)]
    Logs(LogCommands),
    /// Check provider health.
    Health {
        /// Provider name to check (omit to check all).
        name: Option<String>,
    },
    /// View usage reports.
    #[command(subcommand)]
    Usage(UsageCommands),
    /// List available models from configured providers.
    Models,
    /// Keychain/secret selftest.
    Secrets {
        #[command(subcommand)]
        command: SecretsCommands,
    },
}

#[derive(Subcommand, Debug)]
enum ProxyCommands {
    /// Pause the proxy (block new requests).
    Pause,
    /// Resume the proxy.
    Resume,
    /// Toggle the proxy state and print the new state.
    Toggle,
}

#[derive(Subcommand, Debug)]
enum ProviderCommands {
    /// List configured providers.
    List,
    /// Add a new provider.
    Add {
        /// Provider name.
        name: String,
        /// Base URL, e.g. https://api.openai.com/v1.
        base_url: String,
        /// API format: openai, anthropic, google, responses.
        format: String,
        /// API key for the provider.
        #[arg(short, long)]
        key: String,
        /// Authentication scheme.
        #[arg(short, long, default_value = "bearer")]
        auth: String,
        /// Make this the default provider for its format family.
        #[arg(long)]
        default: bool,
        /// Model mapping as local=remote:cost_in:cost_out:cost_cached.
        /// Example: gpt-4o=gpt-4o:5.0:15.0:2.5
        #[arg(short, long = "model")]
        models: Vec<String>,
        /// Fallback provider ID.
        #[arg(long)]
        fallback_id: Option<i64>,
        /// Extra header as KEY=VALUE.
        #[arg(short = 'H', long = "header")]
        extra_headers: Vec<String>,
    },
    /// Delete a provider by ID.
    Delete {
        /// Provider ID.
        id: i64,
    },
    /// Set or update the stored API key for a provider.
    SetKey {
        /// Provider name.
        name: String,
        /// API key.
        key: String,
    },
    /// Delete the stored API key for a provider.
    DeleteKey {
        /// Provider name.
        name: String,
    },
    /// Update a provider by ID.
    Update {
        /// Provider ID.
        id: i64,
        /// New provider name.
        #[arg(short, long)]
        name: Option<String>,
        /// New base URL.
        #[arg(short, long)]
        base_url: Option<String>,
        /// New API format.
        #[arg(short, long)]
        format: Option<String>,
        /// New API key.
        #[arg(short, long)]
        key: Option<String>,
        /// New authentication scheme.
        #[arg(short, long)]
        auth: Option<String>,
        /// Set as default for the format family.
        #[arg(long)]
        default: Option<bool>,
        /// Replace all model mappings.
        #[arg(short, long = "model")]
        models: Vec<String>,
        /// Fallback provider ID.
        #[arg(long)]
        fallback_id: Option<i64>,
        /// Replace all extra headers.
        #[arg(short = 'H', long = "header")]
        extra_headers: Vec<String>,
        /// Clear the stored API key.
        #[arg(long)]
        clear_key: bool,
    },
    /// Refresh available models for a provider.
    RefreshModels {
        /// Provider ID.
        id: i64,
    },
}

#[derive(Subcommand, Debug)]
enum ProjectCommands {
    /// List configured projects.
    List,
    /// Add a new project.
    Add {
        /// Project name.
        name: String,
        /// Label key used by clients.
        label_key: String,
        /// Budget cap (0 disables).
        #[arg(short, long, default_value = "0")]
        budget: f64,
        /// Budget period: daily, weekly, monthly.
        #[arg(long, default_value = "daily")]
        budget_period: String,
        /// Action when budget exceeded: warn, block, pause.
        #[arg(long, default_value = "warn")]
        budget_action: String,
    },
    /// Delete a project by ID.
    Delete {
        /// Project ID.
        id: i64,
    },
}

#[derive(Subcommand, Debug)]
enum LimitCommands {
    /// List configured limits.
    List,
    /// Show current limit status (used vs cap).
    Status,
    /// Add a new limit.
    Add {
        /// Limit name.
        name: String,
        /// Metric: money, tokens, requests, time, rpm, tpm.
        metric: String,
        /// Cap value.
        cap: f64,
        /// Action when exceeded: warn, block, pause.
        #[arg(default_value = "warn")]
        action: String,
        /// Period: once, hourly, daily, weekly, monthly, or custom_sec:<seconds>.
        #[arg(short, long, default_value = "daily")]
        period: String,
        /// Warning threshold as a ratio (0.0-1.0).
        #[arg(short, long, default_value = "0.8")]
        warning_threshold: f64,
        /// Scope: global, provider, project.
        #[arg(short, long, default_value = "global")]
        scope: String,
        /// Scope ID when scope is provider or project.
        #[arg(long)]
        scope_id: Option<i64>,
        /// Active hours start (HH:MM).
        #[arg(long)]
        active_hours_start: Option<String>,
        /// Active hours end (HH:MM).
        #[arg(long)]
        active_hours_end: Option<String>,
        /// Active days bitmask (bit 0 = Monday .. bit 6 = Sunday). 127 = all days.
        #[arg(long, default_value = "127")]
        active_days: u8,
        /// Disable the limit on creation.
        #[arg(long)]
        disabled: bool,
    },
    /// Update an existing limit by ID.
    Update {
        /// Limit ID.
        id: i64,
        /// New name.
        #[arg(short, long)]
        name: Option<String>,
        /// New metric.
        #[arg(short, long)]
        metric: Option<String>,
        /// New cap value.
        #[arg(short, long)]
        cap: Option<f64>,
        /// New action: warn, block, pause.
        #[arg(short, long)]
        action: Option<String>,
        /// New period.
        #[arg(short, long)]
        period: Option<String>,
        /// New warning threshold ratio.
        #[arg(long)]
        warning_threshold: Option<f64>,
        /// Enable or disable the limit.
        #[arg(short, long)]
        enabled: Option<bool>,
        /// New scope.
        #[arg(short, long)]
        scope: Option<String>,
        /// New scope ID.
        #[arg(long)]
        scope_id: Option<i64>,
        /// Active hours start (HH:MM).
        #[arg(long)]
        active_hours_start: Option<String>,
        /// Active hours end (HH:MM).
        #[arg(long)]
        active_hours_end: Option<String>,
        /// Active days bitmask.
        #[arg(long)]
        active_days: Option<u8>,
    },
    /// Delete a limit by ID.
    Delete {
        /// Limit ID.
        id: i64,
    },
}

#[derive(Subcommand, Debug)]
enum SettingsCommands {
    /// Show current settings.
    Show,
    /// Set proxy port.
    SetPort { port: u16 },
    /// Expose proxy to LAN.
    SetExposeToLan { expose: bool },
    /// Set global budget.
    SetBudget { budget: f64 },
    /// Set log retention in days (0 disables cleanup).
    SetLogRetention { days: u32 },
    /// Set webhook URL.
    SetWebhook { url: String },
    /// Test the webhook.
    TestWebhook,
    /// Configure scheduled usage export.
    AutoExport {
        #[command(subcommand)]
        command: AutoExportCommands,
    },
    /// Clean old logs immediately according to retention policy.
    CleanupLogs,
    /// Set auto-update check interval in minutes (0 disables).
    SetAutoUpdateInterval { minutes: u32 },
    /// Mark onboarding as completed.
    CompleteOnboarding,
}

#[derive(Subcommand, Debug)]
enum AutoExportCommands {
    /// Set auto-export folder and interval.
    Set { days: u32, folder: String },
    /// Run usage export now.
    RunNow,
}

#[derive(Subcommand, Debug)]
enum LicenseCommands {
    /// Show current license status.
    Show,
    /// Activate with a license key.
    Activate { key: String },
    /// Deactivate the current license.
    Deactivate,
    /// Print this device fingerprint.
    Fingerprint,
    /// List devices registered to this license.
    Devices,
}

#[derive(Subcommand, Debug)]
enum UpdateCommands {
    /// Check for the latest release.
    Check,
    /// Download the latest CLI binary for this platform.
    Download {
        /// Output path for the downloaded binary.
        #[arg(short, long, default_value = "tokenguard.new")]
        output: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum BackupCommands {
    /// Create a database backup.
    Create { output: PathBuf },
    /// Schedule a restore from a backup file on next start.
    Restore { source: PathBuf },
}

#[derive(Subcommand, Debug)]
enum LogCommands {
    /// Export request logs to CSV.
    Export {
        /// Output CSV file.
        output: PathBuf,
        /// Maximum number of rows.
        #[arg(short, long, default_value = "10000")]
        limit: u64,
        /// Number of days back to include (0 = all).
        #[arg(short, long)]
        days: Option<u64>,
    },
    /// Export audit events to CSV.
    Audit {
        /// Output CSV file.
        output: PathBuf,
        /// Number of days back to include.
        #[arg(short, long, default_value = "30")]
        days: u32,
    },
    /// Query logs with filters.
    Query {
        /// Filter by provider name.
        #[arg(short, long)]
        provider: Option<String>,
        /// Filter by model name.
        #[arg(short, long)]
        model: Option<String>,
        /// Filter by project tag.
        #[arg(short, long)]
        project: Option<String>,
        /// Start timestamp (RFC3339 or date).
        #[arg(long)]
        start: Option<String>,
        /// End timestamp (RFC3339 or date).
        #[arg(long)]
        end: Option<String>,
        /// Page number (1-based).
        #[arg(short, long, default_value = "1")]
        page: u64,
        /// Page size.
        #[arg(long, default_value = "50")]
        page_size: u64,
    },
}

#[derive(Subcommand, Debug)]
enum UsageCommands {
    /// Daily usage for a provider.
    Provider {
        name: String,
        #[arg(short, long, default_value = "30")]
        days: u64,
    },
    /// Daily usage for a project.
    Project {
        tag: String,
        #[arg(short, long, default_value = "30")]
        days: u64,
    },
    /// Total usage per project.
    Totals {
        #[arg(short, long, default_value = "30")]
        days: u64,
    },
    /// Monthly usage summary.
    Monthly {
        #[arg(short, long, default_value = "12")]
        months: u32,
    },
}

#[derive(Subcommand, Debug)]
enum SecretsCommands {
    /// Test the OS keychain backend.
    Selftest,
}

fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("tokenguard"))
}

fn init_state(data_dir: Option<PathBuf>) -> Result<Arc<crate::state::AppState>> {
    let data_dir = data_dir
        .or_else(default_data_dir)
        .context("could not determine data directory; pass --data-dir")?;
    let backend::BackendInit { state, .. } =
        backend::init_backend(data_dir, None::<Box<dyn Notifier + Send + Sync>>)?;
    Ok(state)
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt().try_init();
    }

    match cli.command.unwrap_or(Commands::Start {
        port: None,
        expose_to_lan: false,
    }) {
        Commands::Start {
            port,
            expose_to_lan,
        } => start(cli.data_dir, port, expose_to_lan).await,
        Commands::Status => status(cli.data_dir).await,
        Commands::Proxy(cmd) => proxy_cmd(cli.data_dir, cmd).await,
        Commands::Provider(cmd) => provider(cli.data_dir, cmd).await,
        Commands::Project(cmd) => project(cli.data_dir, cmd).await,
        Commands::Limit(cmd) => limit(cli.data_dir, cmd).await,
        Commands::Settings(cmd) => settings(cli.data_dir, cmd).await,
        Commands::License(cmd) => license_cmd(cmd).await,
        Commands::Update { command } => update_cmd(command).await,
        Commands::Backup(cmd) => backup(cli.data_dir, cmd).await,
        Commands::Logs(cmd) => logs(cli.data_dir, cmd).await,
        Commands::Health { name } => health_check(cli.data_dir, name).await,
        Commands::Usage(cmd) => usage(cli.data_dir, cmd).await,
        Commands::Models => models_cmd(cli.data_dir).await,
        Commands::Secrets { command } => secrets_cmd(command).await,
    }
}

async fn start(data_dir: Option<PathBuf>, port: Option<u16>, expose_to_lan: bool) -> Result<()> {
    let backend::BackendInit {
        state,
        port: db_port,
        expose_to_lan: db_expose,
    } = backend::init_backend(
        data_dir
            .or_else(default_data_dir)
            .context("could not determine data directory; pass --data-dir")?,
        None::<Box<dyn Notifier + Send + Sync>>,
    )?;

    let port = port.unwrap_or(db_port);
    let expose_to_lan = expose_to_lan || db_expose;

    tracing::info!("Token Guard CLI proxy on http://127.0.0.1:{port}");
    if expose_to_lan {
        tracing::info!("proxy exposed to LAN on http://0.0.0.0:{port}");
    }

    let mut proxy_handle = tokio::spawn(backend::serve_proxy(state.clone(), port, expose_to_lan));

    tokio::select! {
        result = &mut proxy_handle => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!("proxy server error: {e}"),
                Err(e) => tracing::error!("proxy task panicked: {e}"),
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Ctrl+C received, shutting down");
            state.shutdown();
            if let Err(e) = proxy_handle.await {
                tracing::error!("proxy task error during shutdown: {e}");
            }
        }
    }

    Ok(())
}

async fn status(data_dir: Option<PathBuf>) -> Result<()> {
    let state = init_state(data_dir)?;
    let spend = state.today_spend();
    let paused = state.paused.load(std::sync::atomic::Ordering::Relaxed);
    let cfg = state.config.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Token Guard status");
    println!("  version: {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  data dir: {}",
        state.db_path.parent().unwrap_or(&state.db_path).display()
    );
    println!("  today spend: ${spend:.2}");
    println!("  budget: ${:.2}", cfg.budget);
    println!("  proxy: {}", if paused { "paused" } else { "active" });
    println!("  providers: {}", cfg.providers.len());
    println!("  projects: {}", cfg.projects.len());
    println!("  limits: {}", cfg.limits.len());
    Ok(())
}

async fn proxy_cmd(data_dir: Option<PathBuf>, cmd: ProxyCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        ProxyCommands::Pause => {
            state.set_paused(true);
            println!("Proxy paused");
        }
        ProxyCommands::Resume => {
            state.set_paused(false);
            println!("Proxy resumed");
        }
        ProxyCommands::Toggle => {
            let paused = state.toggle_pause();
            println!("Proxy {}", if paused { "paused" } else { "resumed" });
        }
    }
    Ok(())
}

async fn provider(data_dir: Option<PathBuf>, cmd: ProviderCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        ProviderCommands::List => providers::list(&state),
        ProviderCommands::Add {
            name,
            base_url,
            format,
            key,
            auth,
            default,
            models,
            fallback_id,
            extra_headers,
        } => providers::add(
            &state,
            name,
            base_url,
            format,
            key,
            auth,
            default,
            models,
            fallback_id,
            extra_headers,
        ),
        ProviderCommands::Delete { id } => providers::delete(&state, id),
        ProviderCommands::SetKey { name, key } => providers::set_key(&state, name, key),
        ProviderCommands::DeleteKey { name } => providers::delete_key(&state, name),
        ProviderCommands::Update {
            id,
            name,
            base_url,
            format,
            key,
            auth,
            default,
            models,
            fallback_id,
            extra_headers,
            clear_key,
        } => providers::update(
            &state,
            id,
            name,
            base_url,
            format,
            key,
            auth,
            default,
            models,
            fallback_id,
            extra_headers,
            clear_key,
        ),
        ProviderCommands::RefreshModels { id } => providers::refresh_models(&state, id).await,
    }
}

async fn project(data_dir: Option<PathBuf>, cmd: ProjectCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        ProjectCommands::List => projects::list(&state),
        ProjectCommands::Add {
            name,
            label_key,
            budget,
            budget_period,
            budget_action,
        } => projects::add(
            &state,
            name,
            label_key,
            budget,
            budget_period,
            budget_action,
        ),
        ProjectCommands::Delete { id } => projects::delete(&state, id),
    }
}

async fn limit(data_dir: Option<PathBuf>, cmd: LimitCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        LimitCommands::List => limits::list(&state),
        LimitCommands::Status => limits::status(&state),
        LimitCommands::Add {
            name,
            metric,
            cap,
            action,
            period,
            warning_threshold,
            scope,
            scope_id,
            active_hours_start,
            active_hours_end,
            active_days,
            disabled,
        } => limits::add(
            &state,
            name,
            metric,
            cap,
            action,
            period,
            warning_threshold,
            scope,
            scope_id,
            active_hours_start,
            active_hours_end,
            active_days,
            !disabled,
        ),
        LimitCommands::Update {
            id,
            name,
            metric,
            cap,
            action,
            period,
            warning_threshold,
            enabled,
            scope,
            scope_id,
            active_hours_start,
            active_hours_end,
            active_days,
        } => limits::update(
            &state,
            id,
            name,
            metric,
            cap,
            action,
            period,
            warning_threshold,
            enabled,
            scope,
            scope_id,
            active_hours_start,
            active_hours_end,
            active_days,
        ),
        LimitCommands::Delete { id } => limits::delete(&state, id),
    }
}

async fn settings(data_dir: Option<PathBuf>, cmd: SettingsCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        SettingsCommands::Show => settings::show(&state),
        SettingsCommands::SetPort { port } => settings::set_port(&state, port),
        SettingsCommands::SetExposeToLan { expose } => settings::set_expose_to_lan(&state, expose),
        SettingsCommands::SetBudget { budget } => settings::set_budget(&state, budget),
        SettingsCommands::SetLogRetention { days } => settings::set_log_retention(&state, days),
        SettingsCommands::SetWebhook { url } => settings::set_webhook(&state, url),
        SettingsCommands::TestWebhook => settings::test_webhook(&state).await,
        SettingsCommands::AutoExport { command } => match command {
            AutoExportCommands::Set { days, folder } => {
                settings::set_auto_export(&state, days, folder)
            }
            AutoExportCommands::RunNow => settings::run_auto_export_now(&state),
        },
        SettingsCommands::CleanupLogs => settings::cleanup_logs(&state),
        SettingsCommands::SetAutoUpdateInterval { minutes } => {
            settings::set_auto_update_interval(&state, minutes)
        }
        SettingsCommands::CompleteOnboarding => settings::complete_onboarding(&state),
    }
}

async fn license_cmd(cmd: LicenseCommands) -> Result<()> {
    match cmd {
        LicenseCommands::Show => license::show(),
        LicenseCommands::Activate { key } => license::activate(key),
        LicenseCommands::Deactivate => license::deactivate(),
        LicenseCommands::Fingerprint => license::fingerprint(),
        LicenseCommands::Devices => license::devices().await,
    }
}

async fn update_cmd(command: Option<UpdateCommands>) -> Result<()> {
    match command.unwrap_or(UpdateCommands::Check) {
        UpdateCommands::Check => update::check().await,
        UpdateCommands::Download { output } => update::download(output).await,
    }
}

async fn backup(data_dir: Option<PathBuf>, cmd: BackupCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        BackupCommands::Create { output } => backup::create(&state, output),
        BackupCommands::Restore { source } => backup::schedule_restore(&state, source),
    }
}

async fn logs(data_dir: Option<PathBuf>, cmd: LogCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        LogCommands::Export {
            output,
            limit,
            days,
        } => logs::export_logs(&state, output, limit, days),
        LogCommands::Audit { output, days } => logs::export_audit(&state, output, days),
        LogCommands::Query {
            provider,
            model,
            project,
            start,
            end,
            page,
            page_size,
        } => logs::query(
            &state, provider, model, project, start, end, page, page_size,
        ),
    }
}

async fn health_check(data_dir: Option<PathBuf>, name: Option<String>) -> Result<()> {
    let state = init_state(data_dir)?;
    health::check(&state, name).await
}

async fn usage(data_dir: Option<PathBuf>, cmd: UsageCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        UsageCommands::Provider { name, days } => usage::provider(&state, name, days),
        UsageCommands::Project { tag, days } => usage::project(&state, tag, days),
        UsageCommands::Totals { days } => usage::totals(&state, days),
        UsageCommands::Monthly { months } => usage::monthly(&state, months),
    }
}

async fn models_cmd(data_dir: Option<PathBuf>) -> Result<()> {
    let state = init_state(data_dir)?;
    models::list(&state)
}

async fn secrets_cmd(command: SecretsCommands) -> Result<()> {
    match command {
        SecretsCommands::Selftest => secrets::selftest(),
    }
}
