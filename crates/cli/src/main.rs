mod config;
mod state;
mod sync;

use anyhow::{Context as _, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use config::{ConfiguredSource, ContextConfig, FileConfig};
use serde::Serialize;
use serde_json::json;
use sessiondb_client::Client;
use sessiondb_types::*;
use state::LocalState;
use std::{path::PathBuf, process::ExitCode, time::Duration};
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
    #[arg(long, global = true)]
    context: Option<String>,
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
    Context {
        #[command(subcommand)]
        command: ContextCommand,
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
    Sync {
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
enum ContextCommand {
    Create { name: String },
    List,
    Show,
    Use { name: String },
    Remove { name: String },
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
    Status,
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

struct Runtime {
    client: Client,
    context: String,
}
fn selected_context<'a>(args: &'a Cli, config: &'a FileConfig) -> &'a str {
    args.context
        .as_deref()
        .or(config.current_context.as_deref())
        .unwrap_or("default")
}
fn server<'a>(args: &'a Cli, config: &'a FileConfig) -> Result<&'a str> {
    args.server
        .as_deref()
        .or_else(|| {
            config
                .contexts
                .get(selected_context(args, config))
                .map(|c| c.server.as_str())
        })
        .or(config.server.as_deref())
        .context("select a server with --server or sessiondb context create")
}
async fn runtime(args: &Cli, config: &FileConfig) -> Result<Runtime> {
    let context = selected_context(args, config).to_owned();
    let server = server(args, config)?;
    let client = config::authenticated(server, Duration::from_secs(args.timeout)).await?;
    if args.verbose > 0 {
        eprintln!("Using {}", client.server());
    }
    Ok(Runtime { client, context })
}
fn emit(args: &Cli, value: &impl Serialize, human: impl FnOnce()) -> Result<()> {
    if args.json {
        println!("{}", serde_json::to_string(value)?);
    } else {
        human();
    }
    Ok(())
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

#[tokio::main]
async fn main() -> ExitCode {
    let args = Cli::parse();
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let code = if error.downcast_ref::<Interrupted>().is_some() {
                130
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
    let mut config = config::load(&args.config)?;
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
            let value = json!({"config_path":args.config,"state_path":args.state_dir,"context":selected_context(args,&config),"server":server(args,&config).ok(),"sources":config.sources,"sync":config.sync});
            emit(args, &value, || {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                )
            })?;
        }
        Command::Context { command } => context_command(args, &mut config, command).await?,
        Command::Auth { command } => auth_command(args, &mut config, command).await?,
        Command::Sources { command } => source_command(args, &mut config, command).await?,
        Command::Sync { watch, command } => {
            let state = LocalState::open(&args.state_dir).await?;
            if matches!(command, Some(SyncCommand::Status)) {
                let value = state.status().await?;
                emit(args, &value, || {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value).unwrap_or_default()
                    )
                })?;
                return Ok(());
            }
            let _lock = sync::lock(&state.directory)?;
            let device = config::device(&mut config);
            config::save(&args.config, &config)?;
            let runtime = runtime(args, &config).await?;
            let sources: Vec<_> = config
                .sources
                .iter()
                .filter(|s| s.context == runtime.context)
                .collect();
            if sources.is_empty() {
                bail!("no sources enabled for this context; use sessiondb sources add");
            }
            if !config.project_rules.is_empty() && !args.quiet {
                eprintln!(
                    "Project matching is not implemented; collected sessions will be unassigned."
                );
            }
            loop {
                let pass = async {
                    let mut results = Vec::new();
                    let mut first_error = None;
                    for source in &sources {
                        let result = async {
                            let registered =
                                sync::ensure_source(&runtime.client, &state, &device, source)
                                    .await?;
                            sync::source(
                                &runtime.client,
                                &state,
                                &registered,
                                &config.sync,
                                args.quiet,
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
                        for value in &results {
                            println!(
                                "{}: {} file(s) accepted",
                                value["source_id"].as_str().unwrap_or(""),
                                value["accepted_files"]
                            );
                        }
                    })?;
                    if let Some(error) = first_error {
                        return Err(error);
                    }
                    Ok(())
                };
                let result = tokio::select! { result=pass=>result,_=tokio::signal::ctrl_c()=>return Err(Interrupted.into()) };
                if !watch {
                    result?;
                    break;
                }
                if let Err(error) = result {
                    if matches!(error.downcast_ref::<sessiondb_client::Error>(),Some(sessiondb_client::Error::Problem(p)) if matches!(p.status,401|403))
                    {
                        return Err(error);
                    }
                    if !args.quiet {
                        eprintln!("Sync will retry: {error:#}");
                    }
                }
                tokio::select! { _=tokio::time::sleep(Duration::from_secs(config.sync.reconcile_interval_seconds.max(1)))=>{},_=tokio::signal::ctrl_c()=>return Err(Interrupted.into()) }
            }
        }
        Command::Sessions { command } => {
            let runtime = runtime(args, &config).await?;
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
                    let value = runtime.client.sessions(&filter).await?;
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
                    let value = runtime.client.session(*id).await?;
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
                    let mut value = runtime
                        .client
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
                            let content = runtime.client.content(m.id).await?;
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
                        runtime.client.export_to(*id, &mut stdout).await?;
                    } else {
                        let parent = output
                            .parent()
                            .filter(|p| !p.as_os_str().is_empty())
                            .unwrap_or_else(|| std::path::Path::new("."));
                        let temporary = tempfile::NamedTempFile::new_in(parent)
                            .context("create export output")?;
                        let mut file = tokio::fs::File::from_std(temporary.reopen()?);
                        runtime.client.export_to(*id, &mut file).await?;
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
            let runtime = runtime(args, &config).await?;
            match command {
                MessageCommand::Show { id } => {
                    let value = runtime.client.message(*id).await?;
                    emit(args, &value, || show_message(&value))?;
                }
                MessageCommand::Content { id } => {
                    let value = runtime.client.content(*id).await?;
                    emit(args, &value, || {
                        for b in &value.content {
                            println!("{}", serde_json::to_string_pretty(b).unwrap_or_default());
                        }
                    })?;
                }
            }
        }
        Command::Status => {
            let client =
                config::authenticated(server(args, &config)?, Duration::from_secs(args.timeout))
                    .await?;
            let value = json!({"server":client.server(),"info":client.info().await?,"sync":LocalState::open(&args.state_dir).await?.status().await?});
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
async fn context_command(
    args: &Cli,
    config: &mut FileConfig,
    command: &ContextCommand,
) -> Result<()> {
    match command {
        ContextCommand::Create { name } => {
            if name.is_empty()
                || name.chars().any(char::is_whitespace)
                || config.contexts.contains_key(name)
            {
                bail!("context name is invalid or already exists");
            }
            let server = server(args, config)?.to_owned();
            let client = config::authenticated(&server, Duration::from_secs(args.timeout)).await?;
            client.info().await?;
            config.contexts.insert(
                name.clone(),
                ContextConfig {
                    server: client.server().into(),
                },
            );
            if config.current_context.is_none() {
                config.current_context = Some(name.clone());
            }
            config::save(&args.config, config)?;
        }
        ContextCommand::Use { name } => {
            if !config.contexts.contains_key(name) {
                bail!("context does not exist");
            }
            config.current_context = Some(name.clone());
            config::save(&args.config, config)?;
        }
        ContextCommand::Remove { name } => {
            if config.contexts.remove(name).is_none() {
                bail!("context does not exist");
            }
            if config.current_context.as_ref() == Some(name) {
                config.current_context = None;
            }
            config::save(&args.config, config)?;
        }
        ContextCommand::List => emit(args, &config.contexts, || {
            for (name, c) in &config.contexts {
                println!("{}\t{}", clean(name), c.server);
            }
        })?,
        ContextCommand::Show => {
            let name = selected_context(args, config);
            let value = config.contexts.get(name).context("no context selected")?;
            emit(args, value, || {
                println!("{}: {}", clean(name), value.server)
            })?;
        }
    }
    Ok(())
}
async fn auth_command(args: &Cli, config: &mut FileConfig, command: &AuthCommand) -> Result<()> {
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
            config.server = Some(server.clone());
            config::save(&args.config, config)?;
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
    let runtime = runtime(args, config).await?;
    match command {
        SourceCommand::List { page } => {
            let value = runtime.client.sources(&page.options()).await?;
            emit(args, &value, || {
                for s in &value.data {
                    println!("{}\t{}\t{}", s.id, s.harness, clean(&s.root));
                }
            })?;
        }
        SourceCommand::Show { id } => {
            let value = runtime.client.source(*id).await?;
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
                    context: runtime.context.clone(),
                    harness,
                    root,
                };
                let source =
                    sync::ensure_source(&runtime.client, &state, &device, &configured).await?;
                if !config.sources.iter().any(|s| {
                    s.context == configured.context
                        && s.root == configured.root
                        && s.harness == configured.harness
                }) {
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
struct Interrupted;
impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("interrupted; pending synchronization is saved")
    }
}
impl std::error::Error for Interrupted {}
