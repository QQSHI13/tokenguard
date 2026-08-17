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
mod projects;
mod providers;
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
    /// Export logs or audit events.
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
    /// Keychain/secret selftest.
    Secrets {
        #[command(subcommand)]
        command: SecretsCommands,
    },
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
        /// API format: openai, anthropic, google.
        format: String,
        /// API key for the provider.
        #[arg(short, long)]
        key: String,
        /// Make this the default provider for its format family.
        #[arg(long)]
        default: bool,
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
        /// New base URL.
        #[arg(short, long)]
        base_url: Option<String>,
        /// New API format.
        #[arg(short, long)]
        format: Option<String>,
        /// New API key.
        #[arg(short, long)]
        key: Option<String>,
        /// Set as default for the format family.
        #[arg(long)]
        default: Option<bool>,
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
    },
    /// Update an existing limit by ID.
    Update {
        /// Limit ID.
        id: i64,
        /// New cap value.
        #[arg(short, long)]
        cap: Option<f64>,
        /// New action: warn, block, pause.
        #[arg(short, long)]
        action: Option<String>,
        /// Enable or disable the limit.
        #[arg(short, long)]
        enabled: Option<bool>,
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
}

#[derive(Subcommand, Debug)]
enum LicenseCommands {
    /// Show current license status.
    Show,
    /// Activate with a license key.
    Activate { key: String },
    /// Deactivate the current license.
    Deactivate,
}

#[derive(Subcommand, Debug)]
enum UpdateCommands {
    /// Check for the latest stable release.
    Check,
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
    },
    /// Export audit events to CSV.
    Audit {
        /// Output CSV file.
        output: PathBuf,
        /// Number of days back to include.
        #[arg(short, long, default_value = "30")]
        days: u32,
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
        Commands::Provider(cmd) => provider(cli.data_dir, cmd).await,
        Commands::Project(cmd) => project(cli.data_dir, cmd).await,
        Commands::Limit(cmd) => limit(cli.data_dir, cmd).await,
        Commands::Settings(cmd) => settings(cli.data_dir, cmd).await,
        Commands::License(cmd) => license_cmd(cli.data_dir, cmd).await,
        Commands::Update { command } => update_cmd(command).await,
        Commands::Backup(cmd) => backup(cli.data_dir, cmd).await,
        Commands::Logs(cmd) => logs(cli.data_dir, cmd).await,
        Commands::Health { name } => health_check(cli.data_dir, name).await,
        Commands::Usage(cmd) => usage(cli.data_dir, cmd).await,
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
    println!("Token Guard status");
    println!("  version: {}", env!("CARGO_PKG_VERSION"));
    println!("  today spend: ${spend:.2}");
    println!("  proxy: {}", if paused { "paused" } else { "active" });
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
            default,
        } => providers::add(&state, name, base_url, format, key, default),
        ProviderCommands::Delete { id } => providers::delete(&state, id),
        ProviderCommands::SetKey { name, key } => providers::set_key(&state, name, key),
        ProviderCommands::DeleteKey { name } => providers::delete_key(&state, name),
        ProviderCommands::Update {
            id,
            base_url,
            format,
            key,
            default,
        } => providers::update(&state, id, base_url, format, key, default),
        ProviderCommands::RefreshModels { id } => providers::refresh_models(&state, id).await,
    }
}

async fn project(data_dir: Option<PathBuf>, cmd: ProjectCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        ProjectCommands::List => projects::list(&state),
        ProjectCommands::Add { name, label_key } => projects::add(&state, name, label_key),
        ProjectCommands::Delete { id } => projects::delete(&state, id),
    }
}

async fn limit(data_dir: Option<PathBuf>, cmd: LimitCommands) -> Result<()> {
    let state = init_state(data_dir)?;
    match cmd {
        LimitCommands::List => limits::list(&state),
        LimitCommands::Add {
            name,
            metric,
            cap,
            action,
        } => limits::add(&state, name, metric, cap, action),
        LimitCommands::Update {
            id,
            cap,
            action,
            enabled,
        } => limits::update(&state, id, cap, action, enabled),
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
    }
}

async fn license_cmd(_data_dir: Option<PathBuf>, cmd: LicenseCommands) -> Result<()> {
    match cmd {
        LicenseCommands::Show => license::show(),
        LicenseCommands::Activate { key } => license::activate(key),
        LicenseCommands::Deactivate => license::deactivate(),
    }
}

async fn update_cmd(command: Option<UpdateCommands>) -> Result<()> {
    match command.unwrap_or(UpdateCommands::Check) {
        UpdateCommands::Check => update::check().await,
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
        LogCommands::Export { output, limit } => logs::export_logs(&state, output, limit),
        LogCommands::Audit { output, days } => logs::export_audit(&state, output, days),
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

async fn secrets_cmd(command: SecretsCommands) -> Result<()> {
    match command {
        SecretsCommands::Selftest => secrets::selftest(),
    }
}
