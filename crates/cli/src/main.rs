mod config;
mod service;
mod state;
mod sync;

use anyhow::{Context as _, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use config::{ConfiguredSource, FileConfig};
use serde::Serialize;
use serde_json::json;
use sessiondb_client::Client;
use sessiondb_types::*;
use state::LocalState;
use std::{collections::HashMap, path::PathBuf, process::ExitCode, time::Duration};
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
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Verify access to a server and save it as the default.
    Connect {
        #[arg(value_name = "SERVER_URL")]
        url: String,
    },
    Sources {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Sessions {
        #[command(subcommand)]
        command: SessionCommand,
    },
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
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    Version,
}
#[derive(Subcommand)]
enum AuthCommand {
    Login {
        /// Read the instance API key from stdin and save it in the OS credential store.
        #[arg(long, required = true)]
        key_stdin: bool,
    },
    Logout,
    Status,
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
    Show { id: Uuid },
    Content { id: Uuid },
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
async fn client_for(args: &Cli, config: &FileConfig) -> Result<Client> {
    let client =
        config::authenticated(server(args, config)?, Duration::from_secs(args.timeout)).await?;
    if args.verbose > 0 {
        eprintln!("Using {}", client.server());
    }
    Ok(client)
}

async fn load_config(args: &Cli) -> Result<FileConfig> {
    config::load(&args.config, &args.state_dir, args.server.as_deref()).await
}
fn emit(args: &Cli, value: &impl Serialize, human: impl FnOnce()) -> Result<()> {
    if args.json {
        println!("{}", serde_json::to_string(value)?);
    } else {
        human();
    }
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
            return emit(args, &value, || {
                println!(
                    "Background sync  {}",
                    value["service"]["state"].as_str().unwrap_or("unsupported")
                );
                println!(
                    "Accepted files   {}\nPending uploads  {}\nKnown rejections {}",
                    value["accepted_files"], value["pending_files"], value["rejected_files"]
                );
                if let Some(at) = value["last_watch_pass"]["finished_at"].as_str() {
                    println!("Last watch pass  {at}");
                    if let Some(url) = value["last_watch_pass"]["server"].as_str() {
                        println!("Watch target     {url}");
                    }
                    if let Some(error) = value["last_watch_pass"]["error"].as_str() {
                        println!("Last pass error  {error}");
                    }
                }
                println!(
                    "State file       {}",
                    value["state_path"].as_str().unwrap_or_default()
                );
            });
        }
        SyncCommand::Retry | SyncCommand::Rejected => {
            let config = load_config(args).await?;
            let state = LocalState::open(&args.state_dir).await?;
            let server = config::normalize_server(server(args, &config)?)?;
            if matches!(cmd, SyncCommand::Retry) {
                let count = state.retry_rejections(&server).await?;
                return emit(args, &json!({"server":server,"cleared":count}), || {
                    println!("{count} remembered rejection(s) cleared for the next pass")
                });
            }
            let entries = state.list_rejections(&server).await?;
            return emit(args, &json!({"server":server,"files":entries}), || {
                for entry in &entries {
                    println!(
                        "{}  {}  {}",
                        entry.source_id,
                        clean(&entry.path),
                        clean(&entry.reason)
                    );
                }
                println!("{} remembered rejection(s)", entries.len());
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
    emit(args, &status, || {
        println!("Background sync  {} ({})", status.state, status.name)
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
fn show_message(m: &Message) {
    println!("{} [{}]", m.role, m.id);
    for block in &m.content {
        match block {
            ContentBlock::Full(
                FullContentBlock::Text { text } | FullContentBlock::Reasoning { text, .. },
            ) => println!("{}", clean(text)),
            other => println!("{}", serde_json::to_string(other).unwrap_or_default()),
        }
    }
    if m.truncated {
        println!(
            "[Content shortened; use sessiondb messages content {}]",
            m.id
        );
    }
    println!();
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

#[tokio::main]
async fn main() -> ExitCode {
    let args = Cli::parse_from(service_upgrade_args(std::env::args_os().collect()));
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let code = if let Some(signal) = error.downcast_ref::<Interrupted>() {
                signal.0
            } else {
                match error.downcast_ref::<sessiondb_client::Error>() {
                    Some(sessiondb_client::Error::Problem(p)) if matches!(p.status, 401 | 403) => 3,
                    Some(sessiondb_client::Error::Problem(p)) if p.status == 404 => 4,
                    Some(sessiondb_client::Error::Authentication(_)) => 3,
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
        Command::Completion { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "sessiondb",
                &mut std::io::stdout(),
            );
        }
        Command::Version => emit(args, &json!({"version":env!("CARGO_PKG_VERSION")}), || {
            println!("sessiondb {}", env!("CARGO_PKG_VERSION"))
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
            let value = json!({"config_path":args.config,"state_path":args.state_dir,"server":selected,"sources":config.sources,"sync":config.sync});
            emit(args, &value, || {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                )
            })?;
        }
        Command::Connect { url } => {
            let client = config::authenticated(url, Duration::from_secs(args.timeout)).await?;
            client.info().await?;
            let _writer = config::write_lock(&args.config)?;
            let mut config = load_config(args).await?;
            config.server = Some(client.server().to_owned());
            config::save(&args.config, &config)?;
            emit(args, &json!({"server":client.server()}), || {
                println!("Default server set to {}", client.server())
            })?;
        }
        Command::Auth { command } => auth_command(args, &load_config(args).await?, command).await?,
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
            let client = client_for(args, &config).await?;
            let mut watched_files = HashMap::new();
            let shutdown = sync_shutdown();
            tokio::pin!(shutdown);
            if !config.project_rules.is_empty() && !args.quiet {
                eprintln!(
                    "Project matching is not implemented; collected sessions will be unassigned."
                );
            }
            loop {
                let pass = async {
                    if *watch {
                        config = load_config(args).await?;
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
                    emit(args, &json!({"sources":results}), || {
                        for report in results.iter().filter(|report| {
                            !watch
                                || report.accepted_files > 0
                                || !report.rejections.is_empty()
                                || !report.retryable_errors.is_empty()
                        }) {
                            println!(
                                "{}: {} file(s) accepted",
                                report.source_id, report.accepted_files
                            );
                        }
                    })?;
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
                    Ok(())
                };
                let result = tokio::select! { result=pass=>result,signal=&mut shutdown=>return Err(signal?.into()) };
                if !watch {
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
                    if matches!(error.downcast_ref::<sessiondb_client::Error>(),Some(sessiondb_client::Error::Problem(p)) if matches!(p.status,401|403))
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
            let client = client_for(args, &load_config(args).await?).await?;
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
                    emit(args, &value, || {
                        for s in &value.data {
                            println!(
                                "{}\t{}\t{}",
                                s.id,
                                s.harness,
                                clean(&s.title).replace('\n', " ")
                            );
                        }
                    })?;
                }
                SessionCommand::Show { id } => {
                    let value = client.session(*id).await?;
                    emit(args, &value, || {
                        println!(
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
                    emit(args, &value, || {
                        for m in &value.data {
                            show_message(m);
                        }
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
                        client.export_to(*id, &mut stdout).await?;
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
        Command::Messages { command } => {
            let client = client_for(args, &load_config(args).await?).await?;
            match command {
                MessageCommand::Show { id } => {
                    let value = client.message(*id).await?;
                    emit(args, &value, || show_message(&value))?;
                }
                MessageCommand::Content { id } => {
                    let value = client.content(*id).await?;
                    emit(args, &value, || {
                        for b in &value.content {
                            println!("{}", serde_json::to_string_pretty(b).unwrap_or_default());
                        }
                    })?;
                }
            }
        }
        Command::Status => {
            let config = load_config(args).await?;
            let client =
                config::authenticated(server(args, &config)?, Duration::from_secs(args.timeout))
                    .await?;
            let value = json!({"server":client.server(),"info":client.info().await?,"sync":sync_status(&LocalState::open(&args.state_dir).await?).await?});
            emit(args, &value, || {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                )
            })?;
        }
    }
    Ok(())
}
async fn auth_command(args: &Cli, config: &FileConfig, command: &AuthCommand) -> Result<()> {
    let server = config::normalize_server(server(args, config)?)?;
    match command {
        AuthCommand::Login { key_stdin: _ } => {
            let mut api_key = String::new();
            tokio::io::stdin()
                .take(64 * 1024 + 1)
                .read_to_string(&mut api_key)
                .await
                .context("read API key from stdin")?;
            if api_key.len() > 64 * 1024 {
                bail!("API key input exceeds 64 KiB");
            }
            let api_key = api_key.trim_end_matches(['\r', '\n']).to_owned();
            let client = Client::new(&server, api_key.clone(), Duration::from_secs(args.timeout))?;
            client.info().await?;
            config::store(server.clone(), api_key).await.context(
                "OS credential storage is required; headless clients can use SESSIONDB_API_KEY",
            )?;
            if config.server.is_none() {
                // Keep first-time keyring setup usable as a bootstrap. Saving a key
                // for another origin must leave an existing default in place.
                let _writer = config::write_lock(&args.config)?;
                let mut latest = load_config(args).await?;
                if latest.server.is_none() {
                    latest.server = Some(server.clone());
                    config::save(&args.config, &latest)?;
                }
            }
            emit(args, &json!({"server":server,"saved":true}), || {
                println!("API key saved in the OS credential store.")
            })?;
        }
        AuthCommand::Logout => {
            config::remove_credentials(server).await?;
        }
        AuthCommand::Status => {
            let client = config::authenticated(&server, Duration::from_secs(args.timeout)).await?;
            client.info().await?;
            emit(
                args,
                &json!({"server":client.server(),"authenticated":true}),
                || println!("API key accepted by {}", client.server()),
            )?;
        }
    }
    Ok(())
}
async fn source_command(
    args: &Cli,
    config: &mut FileConfig,
    command: &SourceCommand,
) -> Result<()> {
    let client = client_for(args, config).await?;
    match command {
        SourceCommand::List { page } => {
            let value = client.sources(&page.options()).await?;
            emit(args, &value, || {
                for s in &value.data {
                    println!("{}\t{}\t{}", s.id, s.harness, clean(&s.root));
                }
            })?;
        }
        SourceCommand::Show { id } => {
            let value = client.source(*id).await?;
            emit(args, &value, || {
                println!("{}\n{}\n{}", value.id, value.harness, clean(&value.root))
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
            emit(args, &registered, || {
                for s in &registered {
                    println!("Enabled {} {} ({})", s.harness, clean(&s.root), s.id);
                }
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
