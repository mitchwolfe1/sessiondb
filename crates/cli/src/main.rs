mod config;
mod service;
mod state;
mod sync;

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};
use config::{ConfiguredSource, FileConfig};
use serde::Serialize;
use serde_json::json;
use sessiondb_client::Client;
use sessiondb_types::*;
use state::LocalState;
use std::{
    collections::HashMap,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "sessiondb",
    version,
    about = "Sync and browse coding-agent sessions"
)]
struct Cli {
    #[arg(long, global = true, env = "SESSIONDB_URL")]
    server: Option<String>,
    #[arg(long,global=true,env="SESSIONDB_CONFIG",default_value_os_t=config::default_config_path())]
    config: PathBuf,
    #[arg(long,global=true,env="SESSIONDB_STATE_DIR",default_value_os_t=config::default_state_path())]
    state_dir: PathBuf,
    #[arg(long,global=true,default_value_t=30,value_parser=clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[arg(long, global = true)]
    json: bool,
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    #[arg(short,long,global=true,action=clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Verify access and save the server URL and API key.
    Connect {
        #[arg(value_name = "SERVER_URL")]
        url: String,
        /// Read a replacement key from standard input.
        #[arg(long)]
        key_stdin: bool,
    },
    Sources {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Sessions {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Find exchanges across accepted history.
    Search(SearchArgs),
    Messages {
        #[command(subcommand)]
        command: MessageCommand,
    },
    /// Sync native sessions and manage the background watcher.
    Sync {
        /// Reconcile configured sources until interrupted.
        #[arg(long)]
        watch: bool,
        #[command(subcommand)]
        command: Option<SyncCommand>,
    },
    Status,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Version,
}
#[derive(Subcommand)]
enum SourceCommand {
    List {
        #[command(flatten)]
        page: PageArgs,
    },
    Show {
        id: Uuid,
    },
    Add {
        #[arg(long)]
        detect: bool,
        #[arg(long)]
        harness: Option<Harness>,
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        device_label: Option<String>,
    },
}
#[derive(Subcommand)]
enum SessionCommand {
    List {
        #[command(flatten)]
        page: PageArgs,
        #[arg(long)]
        harness: Option<Harness>,
        #[arg(long)]
        unassigned: bool,
        #[arg(long)]
        project: Option<Uuid>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        after: Option<chrono::DateTime<chrono::Utc>>,
        #[arg(long)]
        before: Option<chrono::DateTime<chrono::Utc>>,
    },
    Show {
        id: Uuid,
    },
    Messages {
        id: Uuid,
        #[command(flatten)]
        page: PageArgs,
        #[arg(long)]
        leaf: Option<Uuid>,
        #[arg(long)]
        full: bool,
    },
    Export {
        id: Uuid,
        #[arg(long, default_value = "native")]
        format: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        force: bool,
    },
}
#[derive(Subcommand)]
enum MessageCommand {
    /// Show a message, optionally with context on its recorded branch.
    Show {
        id: Uuid,
        /// Show up to N messages on each side of the selected message.
        #[arg(short = 'C', long, value_name = "N", value_parser = clap::value_parser!(u32).range(0..=50))]
        context: Option<u32>,
        /// Show up to N earlier messages. Overrides -C on this side.
        #[arg(short = 'B', long, alias = "before-context", value_name = "N", value_parser = clap::value_parser!(u32).range(0..=50))]
        before: Option<u32>,
        /// Show up to N later messages. Overrides -C on this side.
        #[arg(short = 'A', long, alias = "after-context", value_name = "N", value_parser = clap::value_parser!(u32).range(0..=50))]
        after: Option<u32>,
        /// Select a continuation with a child or descendant message ID.
        #[arg(long)]
        leaf: Option<Uuid>,
    },
    Content {
        id: Uuid,
    },
}
#[derive(clap::Args)]
struct SearchArgs {
    /// Ranked keywords. Use AND, OR, quotes or -word for boolean matching.
    query: String,
    #[arg(long, default_value = "keyword")]
    mode: SearchMode,
    #[arg(long, default_value = "relevance")]
    sort: SearchSort,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=50))]
    limit: u32,
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, conflicts_with = "unassigned")]
    project: Vec<Uuid>,
    #[arg(long)]
    unassigned: bool,
    #[arg(long)]
    harness: Vec<Harness>,
    #[arg(long)]
    role: Vec<Role>,
    #[arg(long)]
    tag: Vec<String>,
    #[arg(long)]
    after: Option<chrono::DateTime<chrono::Utc>>,
    #[arg(long)]
    before: Option<chrono::DateTime<chrono::Utc>>,
    #[arg(long)]
    include_tools: bool,
    #[arg(long)]
    tool_name: Option<String>,
}

#[derive(Subcommand)]
enum SyncCommand {
    /// Show the background service, last watch pass, and local sync counts.
    Status,
    /// Install and start a watcher for this server at user login.
    Install,
    /// Start the installed watcher.
    Start,
    /// Stop the watcher for this login session.
    Stop,
    /// Stop the watcher and remove its login service.
    Uninstall,
    /// Read the watcher's recent service logs.
    Logs {
        #[arg(short, long)]
        follow: bool,
    },
    /// List remembered native-file rejections for this server.
    Rejected,
    /// Make the watcher retry unchanged rejected files on its next pass.
    Retry,
}
#[derive(Subcommand)]
enum ConfigCommand {
    Show,
}
#[derive(Args)]
struct PageArgs {
    #[arg(long,default_value_t=50,value_parser=clap::value_parser!(u32).range(1..=200))]
    limit: u32,
    #[arg(long)]
    cursor: Option<String>,
}
impl PageArgs {
    fn options(&self) -> ListOptions {
        ListOptions {
            limit: Some(self.limit),
            cursor: self.cursor.clone(),
            harness: None,
        }
    }
}

fn server<'a>(args: &'a Cli, config: &'a FileConfig) -> Result<&'a str> {
    args.server
        .as_deref()
        .or(config.server.as_deref())
        .context("select a server with sessiondb connect <SERVER_URL> or --server")
}
fn client_for(args: &Cli, config: &FileConfig) -> Result<Client> {
    let url = config::normalize_server(server(args, config)?)?;
    if args.verbose > 0 {
        eprintln!("Using {url}");
    }
    Ok(Client::new(
        &url,
        config.request_key(&url)?,
        Duration::from_secs(args.timeout),
    )?)
}

async fn load_config(args: &Cli) -> Result<FileConfig> {
    config::load(&args.config, &args.state_dir, args.server.as_deref()).await
}

#[derive(Debug)]
struct StdoutError(io::Error);

impl std::fmt::Display for StdoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("write stdout")
    }
}

impl std::error::Error for StdoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

fn emit(
    args: &Cli,
    value: &impl Serialize,
    human: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<()> {
    let mut out = io::stdout().lock();
    let result = if args.json {
        writeln!(out, "{}", serde_json::to_string(value)?)
    } else {
        human(&mut out)
    };
    result.and_then(|()| out.flush()).map_err(StdoutError)?;
    Ok(())
}

async fn sync_status(state: &LocalState) -> Result<serde_json::Value> {
    let mut value = state.status().await?;
    value["service"] = serde_json::to_value(
        service::Service::current(&state.directory)?
            .map(|svc| svc.status())
            .transpose()?,
    )?;
    Ok(value)
}

async fn sync_command(args: &Cli, cmd: &SyncCommand) -> Result<()> {
    match cmd {
        SyncCommand::Status => {
            let state = LocalState::open(&args.state_dir).await?;
            let value = sync_status(&state).await?;
            return emit(args, &value, |out| {
                writeln!(
                    out,
                    "Background sync  {}",
                    value["service"]["state"].as_str().unwrap_or("unsupported")
                )?;
                writeln!(
                    out,
                    "Accepted files   {}\nPending uploads  {}\nKnown rejections {}",
                    value["accepted_files"], value["pending_files"], value["rejected_files"]
                )?;
                if let Some(at) = value["last_watch_pass"]["finished_at"].as_str() {
                    writeln!(out, "Last watch pass  {at}")?;
                    if let Some(url) = value["last_watch_pass"]["server"].as_str() {
                        writeln!(out, "Watch target     {url}")?;
                    }
                    if let Some(error) = value["last_watch_pass"]["error"].as_str() {
                        writeln!(out, "Last pass error  {error}")?;
                    }
                }
                writeln!(
                    out,
                    "State file       {}",
                    value["state_path"].as_str().unwrap_or_default()
                )
            });
        }
        SyncCommand::Retry | SyncCommand::Rejected => {
            let config = load_config(args).await?;
            let state = LocalState::open(&args.state_dir).await?;
            let server = config::normalize_server(server(args, &config)?)?;
            if matches!(cmd, SyncCommand::Retry) {
                let count = state.retry_rejections(&server).await?;
                return emit(args, &json!({"server":server,"cleared":count}), |out| {
                    writeln!(
                        out,
                        "{count} remembered rejection(s) cleared for the next pass"
                    )
                });
            }
            let entries = state.list_rejections(&server).await?;
            return emit(args, &json!({"server":server,"files":entries}), |out| {
                for entry in &entries {
                    writeln!(
                        out,
                        "{}  {}  {}",
                        entry.source_id,
                        clean(&entry.path),
                        clean(&entry.reason)
                    )?;
                }
                writeln!(out, "{} remembered rejection(s)", entries.len())
            });
        }
        _ => {}
    }
    config::private_dir(&args.state_dir)?;
    let directory = args.state_dir.canonicalize()?;
    let svc = service::Service::current(&directory)?
        .context("background sync services require Linux or macOS")?;
    if let SyncCommand::Logs { follow } = cmd {
        if args.json {
            bail!(
                "sync logs streams service log text; use sync status --json for structured status"
            );
        }
        return svc.logs(*follow);
    }
    let _lock = svc.lock()?;
    match cmd {
        SyncCommand::Install => {
            let config = load_config(args).await?;
            let url = config::normalize_server(server(args, &config)?)?;
            let roots: Vec<_> = config.sources.iter().filter(|s| s.server == url).collect();
            if roots.is_empty() {
                bail!("no sources enabled for this server; use sessiondb sources add");
            }
            let mut cwd = None;
            for source in roots {
                if config::expand(&source.root)?.is_relative() {
                    cwd = Some(std::env::current_dir()?);
                    break;
                }
            }
            let command = vec![
                std::env::current_exe()?
                    .into_os_string()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("executable path must be UTF-8"))?,
                format!(
                    "--config={}",
                    std::path::absolute(&args.config)?
                        .to_str()
                        .context("config path must be UTF-8")?
                ),
                format!(
                    "--state-dir={}",
                    directory.to_str().context("state path must be UTF-8")?
                ),
                format!("--server={url}"),
                format!("--timeout={}", args.timeout),
                "sync".into(),
                "--watch".into(),
            ];
            svc.install(&command, cwd.as_deref())?;
        }
        SyncCommand::Start => svc.start()?,
        SyncCommand::Stop => svc.stop()?,
        SyncCommand::Uninstall => svc.uninstall()?,
        _ => unreachable!(),
    }
    let status = svc.status()?;
    emit(args, &status, |out| {
        writeln!(out, "Background sync  {} ({})", status.state, status.name)
    })
}

async fn sync_shutdown() -> Result<Interrupted> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result=tokio::signal::ctrl_c()=>{result?;Ok(Interrupted(130))}
            _=term.recv()=>Ok(Interrupted(143)),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(Interrupted(130))
    }
}
fn clean(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
fn show_message(out: &mut dyn Write, m: &Message) -> io::Result<()> {
    writeln!(out, "{} [{}]", m.role, m.id)?;
    for block in &m.content {
        match block {
            ContentBlock::Full(
                FullContentBlock::Text { text } | FullContentBlock::Reasoning { text, .. },
            ) => writeln!(out, "{}", clean(text)),
            other => writeln!(out, "{}", serde_json::to_string(other).unwrap_or_default()),
        }?;
    }
    if m.truncated {
        writeln!(
            out,
            "[Content shortened; use sessiondb messages content {}]",
            m.id
        )?;
    }
    writeln!(out)
}

// Older login services already pin a URL. Accept their exact generated invocation
// until the next sync install replaces the definition.
fn service_upgrade_args(mut args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    if let [_, config, state, old_name, url, timeout, command, watch] = args.as_slice()
        && config.as_encoded_bytes().starts_with(b"--config=")
        && state.as_encoded_bytes().starts_with(b"--state-dir=")
        && old_name.as_encoded_bytes().starts_with(b"--context=")
        && url.as_encoded_bytes().starts_with(b"--server=")
        && timeout.as_encoded_bytes().starts_with(b"--timeout=")
        && command == "sync"
        && watch == "--watch"
    {
        args.remove(3);
    }
    args
}

fn authentication_failure(error: &sessiondb_client::Error) -> bool {
    match error {
        sessiondb_client::Error::Authentication(_) => true,
        sessiondb_client::Error::Problem(problem) => matches!(problem.status, 401 | 403),
        sessiondb_client::Error::Http(status) => matches!(status.as_u16(), 401 | 403),
        _ => false,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Cli::parse_from(service_upgrade_args(std::env::args_os().collect()));
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error)
            if error
                .downcast_ref::<StdoutError>()
                .is_some_and(|e| e.0.kind() == io::ErrorKind::BrokenPipe) =>
        {
            ExitCode::SUCCESS
        }
        Err(error) => {
            let code = if let Some(signal) = error.downcast_ref::<Interrupted>() {
                signal.0
            } else {
                match error.downcast_ref::<sessiondb_client::Error>() {
                    Some(error) if authentication_failure(error) => 3,
                    Some(sessiondb_client::Error::Problem(p)) if p.status == 404 => 4,
                    _ => 1,
                }
            };
            eprintln!("sessiondb: {error:#}");
            ExitCode::from(code)
        }
    }
}
async fn run(args: &Cli) -> Result<()> {
    if let Command::Sync {
        watch,
        command: Some(cmd),
    } = &args.command
    {
        if *watch {
            bail!("use --watch for foreground sync or choose a sync subcommand");
        }
        return sync_command(args, cmd).await;
    }
    match &args.command {
        Command::Version => emit(args, &json!({"version":env!("CARGO_PKG_VERSION")}), |out| {
            writeln!(out, "sessiondb {}", env!("CARGO_PKG_VERSION"))
        })?,
        Command::Config {
            command: ConfigCommand::Show,
        } => {
            let config = load_config(args).await?;
            let selected = args
                .server
                .as_deref()
                .or(config.server.as_deref())
                .map(config::normalize_server)
                .transpose()?;
            let has_key = selected.as_deref().is_some_and(|url| {
                std::env::var_os("SESSIONDB_API_KEY").is_some() || config.saved_key(url).is_some()
            });
            let value = json!({"config_path":args.config,"state_path":args.state_dir,"server":selected,
                "api_key":has_key.then_some("[redacted]"),"sources":config.sources,"sync":config.sync});
            emit(args, &value, |out| {
                writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                )
            })?;
        }
        Command::Connect { url, key_stdin } => connect(args, url, *key_stdin).await?,
        Command::Sources { command } => {
            let _writer = if matches!(command, SourceCommand::Add { .. }) {
                Some(config::write_lock(&args.config)?)
            } else {
                None
            };
            let mut config = load_config(args).await?;
            source_command(args, &mut config, command).await?;
        }
        Command::Sync { watch, .. } => {
            let mut config = load_config(args).await?;
            let state = LocalState::open(&args.state_dir).await?;
            let _lock = sync::lock(&state.directory)?;
            let device = if let Some(device) = &config.device_label {
                device.clone()
            } else {
                let _writer = config::write_lock(&args.config)?;
                config = load_config(args).await?;
                let device = config::device(&mut config);
                config::save(&args.config, &config)?;
                device
            };
            if *watch {
                state.upgrade_watch_state().await?;
            }
            let target = config::normalize_server(server(args, &config)?)?;
            let timeout = Duration::from_secs(args.timeout);
            let mut key = config.request_key(&target)?;
            let mut client = Client::new(&target, key.clone(), timeout)?;
            if args.verbose > 0 {
                eprintln!("Using {target}");
            }
            let mut watched_files = HashMap::new();
            let shutdown = sync_shutdown();
            tokio::pin!(shutdown);
            if !config.project_rules.is_empty() && !args.quiet {
                eprintln!(
                    "Project matching is not implemented; collected sessions will be unassigned."
                );
            }
            loop {
                let mut output_failed = false;
                let pass = async {
                    if *watch {
                        config = load_config(args).await?;
                        let updated_key = config.request_key(&target)?;
                        if updated_key != key {
                            client = Client::new(&target, updated_key.clone(), timeout)?;
                            key = updated_key;
                        }
                    }
                    let sources: Vec<_> = config
                        .sources
                        .iter()
                        .filter(|s| s.server == client.server())
                        .collect();
                    if sources.is_empty() {
                        bail!("no sources enabled for this server; use sessiondb sources add");
                    }
                    let mut results = Vec::new();
                    let mut first_error = None;
                    let info = client.info().await?;
                    for source in &sources {
                        let result = async {
                            let registered =
                                sync::ensure_source(&client, &state, &device, source).await?;
                            sync::source(
                                &client,
                                &state,
                                &registered,
                                &config.sync,
                                args.quiet,
                                watch.then(|| watched_files.entry(registered.id).or_default()),
                                &info,
                            )
                            .await
                        }
                        .await;
                        match result {
                            Ok(value) => results.push(value),
                            Err(error) => {
                                if !args.quiet {
                                    eprintln!("Source {}: {error:#}", source.root.display());
                                }
                                if first_error.is_none() {
                                    first_error = Some(error);
                                }
                            }
                        }
                    }
                    let output = emit(args, &json!({"sources":results}), |out| {
                        for report in results.iter().filter(|report| {
                            !watch
                                || report.accepted_files > 0
                                || !report.rejections.is_empty()
                                || !report.retryable_errors.is_empty()
                        }) {
                            writeln!(
                                out,
                                "{}: {} file(s) accepted",
                                report.source_id, report.accepted_files
                            )?;
                        }
                        Ok(())
                    });
                    output_failed = output.is_err();
                    // Keep the actual sync failure if the reader also closed the summary pipe.
                    if let Some(error) = first_error {
                        return Err(error);
                    }
                    let rejected: usize = results.iter().map(|r| r.rejections.len()).sum();
                    let pending: usize = results.iter().map(|r| r.retryable_errors.len()).sum();
                    if pending > 0 {
                        bail!(
                            "{pending} file(s) pending after server errors, {rejected} file(s) rejected"
                        );
                    }
                    if rejected > 0 {
                        bail!("{rejected} file(s) rejected");
                    }
                    output
                };
                let result = tokio::select! { result=pass=>result,signal=&mut shutdown=>return Err(signal?.into()) };
                if !watch || output_failed {
                    result?;
                    break;
                }
                state
                    .watch_finished(
                        client.server(),
                        result.as_ref().err().map(|e| format!("{e:#}")),
                    )
                    .await?;
                if let Err(error) = result {
                    if error
                        .downcast_ref::<sessiondb_client::Error>()
                        .is_some_and(authentication_failure)
                    {
                        return Err(error);
                    }
                    if !args.quiet {
                        eprintln!("Sync will retry: {error:#}");
                    }
                }
                tokio::select! { _=tokio::time::sleep(Duration::from_secs(config.sync.reconcile_interval_seconds.max(1)))=>{},signal=&mut shutdown=>return Err(signal?.into()) }
            }
        }
        Command::Sessions { command } => {
            let client = client_for(args, &load_config(args).await?)?;
            match command {
                SessionCommand::List {
                    page,
                    harness,
                    unassigned,
                    project,
                    tag,
                    after,
                    before,
                } => {
                    let filter = SessionFilter {
                        limit: Some(page.limit),
                        cursor: page.cursor.clone(),
                        project_id: *project,
                        unassigned: Some(*unassigned),
                        harness: *harness,
                        tag: tag.clone(),
                        after: *after,
                        before: *before,
                        ..Default::default()
                    };
                    let value = client.sessions(&filter).await?;
                    emit(args, &value, |out| {
                        for s in &value.data {
                            writeln!(
                                out,
                                "{}\t{}\t{}",
                                s.id,
                                s.harness,
                                clean(&s.title).replace('\n', " ")
                            )?;
                        }
                        Ok(())
                    })?;
                }
                SessionCommand::Show { id } => {
                    let value = client.session(*id).await?;
                    emit(args, &value, |out| {
                        writeln!(
                            out,
                            "{}\nID: {}\nHarness: {}\nDirectory: {}\nMessages: {}",
                            clean(&value.title),
                            value.id,
                            value.harness,
                            clean(value.recorded_cwd.as_deref().unwrap_or("unknown")),
                            value.message_count
                        )
                    })?;
                }
                SessionCommand::Messages {
                    id,
                    page,
                    leaf,
                    full,
                } => {
                    let mut value = client
                        .messages(
                            *id,
                            &MessageFilter {
                                limit: Some(page.limit),
                                cursor: page.cursor.clone(),
                                leaf_id: *leaf,
                            },
                        )
                        .await?;
                    if *full {
                        for m in &mut value.data {
                            let content = client.content(m.id).await?;
                            m.content = content
                                .content
                                .into_iter()
                                .map(ContentBlock::Full)
                                .collect();
                            m.truncated = false;
                            m.content_url = None;
                        }
                    }
                    emit(args, &value, |out| {
                        for m in &value.data {
                            show_message(out, m)?;
                        }
                        Ok(())
                    })?;
                }
                SessionCommand::Export {
                    id,
                    format,
                    output,
                    force,
                } => {
                    if format != "native" {
                        bail!("this build supports --format native");
                    }
                    if output.as_os_str() == "-" {
                        let mut stdout = tokio::io::stdout();
                        client
                            .export_to(*id, &mut stdout)
                            .await
                            .map_err(|error| match error {
                                sessiondb_client::Error::Io(e) => {
                                    anyhow::Error::new(StdoutError(e))
                                }
                                other => other.into(),
                            })?;
                    } else {
                        let parent = output
                            .parent()
                            .filter(|p| !p.as_os_str().is_empty())
                            .unwrap_or_else(|| std::path::Path::new("."));
                        let temporary = tempfile::NamedTempFile::new_in(parent)
                            .context("create export output")?;
                        let mut file = tokio::fs::File::from_std(temporary.reopen()?);
                        client.export_to(*id, &mut file).await?;
                        file.sync_all().await?;
                        drop(file);
                        if *force {
                            temporary.persist(output).map_err(|e| e.error)?;
                        } else {
                            temporary
                                .persist_noclobber(output)
                                .map_err(|e| e.error)
                                .context("output already exists; use --force to replace it")?;
                        }
                        if !args.quiet {
                            eprintln!("Saved {}", output.display());
                        }
                    }
                }
            }
        }
        Command::Search(search) => {
            let client = client_for(args, &load_config(args).await?)?;
            let result = client
                .search(&SearchRequest {
                    query: search.query.clone(),
                    mode: search.mode,
                    sort: search.sort,
                    limit: Some(search.limit),
                    cursor: search.cursor.clone(),
                    filters: SearchFilters {
                        project_ids: search.project.clone(),
                        unassigned: search.unassigned,
                        harnesses: search.harness.clone(),
                        roles: search.role.clone(),
                        tags: search.tag.clone(),
                        after: search.after,
                        before: search.before,
                        include_tools: search.include_tools,
                        tool_name: search.tool_name.clone(),
                    },
                })
                .await?;
            emit(args, &result, |out| {
                for hit in &result.data {
                    writeln!(
                        out,
                        "{}  [{}]  {}",
                        clean(&hit.title),
                        hit.session_id,
                        hit.harness
                    )?;
                    write!(out, "{}", hit.role)?;
                    if let Some(name) = &hit.tool_name {
                        write!(out, "  {}", clean(name))?;
                    }
                    if let Some(at) = hit.timestamp {
                        write!(out, "  {}", at.to_rfc3339())?;
                    }
                    writeln!(out, "  {}\n{}", hit.url, clean(&hit.excerpt))?;
                    writeln!(out, "{}", hit.show_command)?;
                    writeln!(out)?;
                }
                if let Some(cursor) = &result.next_cursor {
                    writeln!(out, "More results: --cursor {cursor}")?;
                }
                Ok(())
            })?;
        }
        Command::Messages { command } => {
            let client = client_for(args, &load_config(args).await?)?;
            match command {
                MessageCommand::Show {
                    id,
                    context,
                    before,
                    after,
                    leaf,
                } => {
                    // With only -A or -B, the opposite side has zero context.
                    let width = context.or((before.is_some() || after.is_some()).then_some(0));
                    if width.is_none() && leaf.is_none() {
                        let message = client.message(*id).await?;
                        return emit(args, &message, |out| show_message(out, &message));
                    }
                    let context = client
                        .read_around(
                            *id,
                            &AroundOptions {
                                before: before.or(width),
                                after: after.or(width),
                                leaf_id: *leaf,
                            },
                        )
                        .await?;
                    emit(args, &context, |out| {
                        for message in &context.data {
                            if message.id == context.anchor_id {
                                write!(out, "> ")?;
                            }
                            show_message(out, message)?;
                        }
                        for child in &context.branch_choices {
                            writeln!(out, "Continue with --leaf {child}")?;
                        }
                        Ok(())
                    })?;
                }
                MessageCommand::Content { id } => {
                    let value = client.content(*id).await?;
                    emit(args, &value, |out| {
                        for b in &value.content {
                            writeln!(
                                out,
                                "{}",
                                serde_json::to_string_pretty(b).unwrap_or_default()
                            )?;
                        }
                        Ok(())
                    })?;
                }
            }
        }
        Command::Status => {
            let config = load_config(args).await?;
            let client = client_for(args, &config)?;
            let value = json!({"server":client.server(),"info":client.info().await?,"sync":sync_status(&LocalState::open(&args.state_dir).await?).await?});
            emit(args, &value, |out| {
                writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                )
            })?;
        }
    }
    Ok(())
}
fn prompt_key(origin: &str) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        return Err(sessiondb_client::Error::Authentication(format!(
            "run sessiondb connect {origin} in a terminal, supply --key-stdin, or set SESSIONDB_API_KEY"
        )).into());
    }
    rpassword::prompt_password(format!("API key for {origin}: "))
        .context("read API key from terminal")
}

async fn stdin_key(origin: &str) -> Result<String> {
    if std::io::stdin().is_terminal() {
        return prompt_key(origin);
    }
    const MAX: u64 = 64 * 1024;
    let mut input = String::new();
    tokio::io::stdin()
        .take(MAX + 1)
        .read_to_string(&mut input)
        .await
        .context("read API key from stdin")?;
    if input.len() as u64 > MAX {
        bail!("API key input exceeds 64 KiB");
    }
    Ok(input.trim_end_matches(['\r', '\n']).to_owned())
}

async fn verify_key(origin: &str, key: &str, timeout: Duration) -> sessiondb_client::Result<()> {
    Client::new(origin, key.to_owned(), timeout)?.info().await?;
    Ok(())
}

async fn connect(args: &Cli, url: &str, key_stdin: bool) -> Result<()> {
    let origin = config::normalize_server(url)?;
    let config = load_config(args).await?;
    let explicit = if key_stdin {
        Some(stdin_key(&origin).await?)
    } else {
        config::environment_key()?
    };
    let (mut key, from_disk) = match (explicit, config.saved_key(&origin)) {
        (Some(key), _) => (key, false),
        (None, Some(key)) => (key.to_owned(), true),
        (None, None) => (prompt_key(&origin)?, false),
    };
    let timeout = Duration::from_secs(args.timeout);
    if let Err(error) = verify_key(&origin, &key, timeout).await {
        if !from_disk || !authentication_failure(&error) {
            return Err(error.into());
        }
        if !std::io::stdin().is_terminal() {
            return Err(anyhow::Error::new(error).context(
                "saved API key rejected; reconnect in a terminal or supply a replacement key",
            ));
        }
        eprintln!("Saved API key rejected.");
        key = prompt_key(&origin)?;
        verify_key(&origin, &key, timeout).await?;
    }
    let _lock = config::write_lock(&args.config)?;
    let mut latest = load_config(args).await?;
    latest.remember(origin.clone(), key);
    config::save(&args.config, &latest)?;
    emit(args, &json!({"server":origin}), |out| {
        writeln!(out, "Connected to {origin}")
    })
}

async fn source_command(
    args: &Cli,
    config: &mut FileConfig,
    command: &SourceCommand,
) -> Result<()> {
    let client = client_for(args, config)?;
    match command {
        SourceCommand::List { page } => {
            let value = client.sources(&page.options()).await?;
            emit(args, &value, |out| {
                for s in &value.data {
                    writeln!(out, "{}\t{}\t{}", s.id, s.harness, clean(&s.root))?;
                }
                Ok(())
            })?;
        }
        SourceCommand::Show { id } => {
            let value = client.source(*id).await?;
            emit(args, &value, |out| {
                writeln!(
                    out,
                    "{}\n{}\n{}",
                    value.id,
                    value.harness,
                    clean(&value.root)
                )
            })?;
        }
        SourceCommand::Add {
            detect,
            harness,
            root,
            device_label,
        } => {
            let roots = if *detect {
                if harness.is_some() || root.is_some() {
                    bail!("use --detect or --harness with --root");
                }
                sync::detect()
            } else {
                vec![(
                    harness.context("supply --harness with --root, or --detect")?,
                    root.as_ref().context("supply --root")?.clone(),
                )]
            };
            if roots.is_empty() {
                bail!("no native session stores were found; supply --harness and --root");
            }
            if let Some(label) = device_label {
                config.device_label = Some(label.clone());
            }
            let device = config::device(config);
            let state = LocalState::open(&args.state_dir).await?;
            let mut registered = Vec::new();
            for (harness, root) in roots {
                let root = config::expand(&root)?.canonicalize()?;
                let configured = ConfiguredSource {
                    server: client.server().to_owned(),
                    harness,
                    root,
                };
                let source = sync::ensure_source(&client, &state, &device, &configured).await?;
                if !config.sources.contains(&configured) {
                    config.sources.push(configured);
                }
                registered.push(source);
            }
            config::save(&args.config, config)?;
            emit(args, &registered, |out| {
                for s in &registered {
                    writeln!(out, "Enabled {} {} ({})", s.harness, clean(&s.root), s.id)?;
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}
#[derive(Debug)]
struct Interrupted(u8);
impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("interrupted; pending synchronization is saved")
    }
}
impl std::error::Error for Interrupted {}

#[cfg(test)]
mod connection_cli_tests {
    use super::*;
    use std::ffi::OsString;
    #[test]
    fn an_old_installed_watcher_can_restart_with_its_pinned_url() {
        let old = [
            "sessiondb",
            "--config=/fixture/client.toml",
            "--state-dir=/fixture/state",
            "--context=former-name",
            "--server=http://original.example/",
            "--timeout=45",
            "sync",
            "--watch",
        ];
        let args = Cli::try_parse_from(service_upgrade_args(
            old.iter().map(OsString::from).collect(),
        ))
        .unwrap();
        assert_eq!(args.server.as_deref(), Some("http://original.example/"));
        assert_eq!(args.timeout, 45);
        assert!(matches!(
            args.command,
            Command::Sync {
                watch: true,
                command: None
            }
        ));
        assert!(Cli::try_parse_from(["sessiondb", "context", "use", "old"]).is_err());
        let named = ["sessiondb", "--context=old", "sessions", "list"];
        assert!(
            Cli::try_parse_from(service_upgrade_args(
                named.iter().map(OsString::from).collect()
            ))
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["sessiondb", "connect", "http://host", "--name", "old"]).is_err()
        );
    }
}
