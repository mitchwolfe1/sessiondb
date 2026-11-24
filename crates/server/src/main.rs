use anyhow::Result;
use axum::{Router, extract::State, routing::get};
use clap::{Parser, Subcommand};
use sessiondb_server::{AppState, MIGRATOR, config::Config};
use sqlx::postgres::PgPoolOptions;
use std::{path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::watch};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about = "Store and access coding-agent sessions")]
struct Args {
    #[arg(
        long,
        global = true,
        env = "SESSIONDB_SERVER_CONFIG",
        default_value = "/etc/sessiondb/config.toml"
    )]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Apply catalog migrations explicitly before starting the service.
    Migrate,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sessiondb_server=info"));
    if config.telemetry.log_format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
    if matches!(args.command, Some(Command::Migrate)) {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&config.database_url()?)
            .await?;
        MIGRATOR.run(&pool).await?;
        sqlx::query("SELECT sessiondb_prepare_vector_storage()")
            .execute(&pool)
            .await?;
        tracing::info!("catalog migrations applied");
        return Ok(());
    }
    let state = AppState::connect(config).await?;
    let listener = TcpListener::bind(state.config.listen).await?;
    tracing::info!(address=%listener.local_addr()?,"SessionDB listening");
    let (stop, shutdown) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        if let Err(error) = stop.send(true) {
            tracing::debug!(%error, "API already stopped before shutdown signal");
        }
    });
    if let Some(address) = state.config.telemetry.metrics_listen {
        let listener = TcpListener::bind(address).await?;
        let metrics = Router::new()
            .route("/metrics", get(metrics))
            .with_state(state.clone());
        let mut metrics_stop = shutdown.clone();
        tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, metrics)
                .with_graceful_shutdown(async move {
                    if let Err(error) = metrics_stop.changed().await {
                        tracing::debug!(%error, "metrics shutdown sender closed");
                    }
                })
                .await
            {
                tracing::error!(%error, "metrics listener failed");
            }
        });
    }
    sessiondb_server::serve(state, listener, shutdown).await
}
async fn metrics(State(state): State<Arc<AppState>>) -> impl axum::response::IntoResponse {
    (
        [("content-type", "text/plain; version=0.0.4")],
        format!(
            "# HELP sessiondb_http_requests_total HTTP requests served.\n# TYPE sessiondb_http_requests_total counter\nsessiondb_http_requests_total {}\n",
            state.requests.load(std::sync::atomic::Ordering::Relaxed)
        ),
    )
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
        } else {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
