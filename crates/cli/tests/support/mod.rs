use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

pub struct Services {
    pub postgres: String,
    pub s3: String,
    pub password: String,
    pub access: String,
    pub secret: String,
    volumes: Vec<String>,
}
pub fn docker(args: &[&str]) -> std::process::Output {
    let output = Command::new("docker")
        .args(args)
        .output()
        .expect("Docker is required for this integration test");
    assert!(
        output.status.success(),
        "Docker operation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
pub fn port(container: &str, internal: &str) -> u16 {
    String::from_utf8(docker(&["port", container, internal]).stdout)
        .unwrap()
        .trim()
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
impl Services {
    pub async fn start() -> Self {
        let id = Uuid::new_v4().to_string();
        let mut value = Self {
            postgres: format!("sessiondb-it-pg-{id}"),
            s3: format!("sessiondb-it-s3-{id}"),
            password: Uuid::new_v4().to_string(),
            access: Uuid::new_v4().to_string(),
            secret: Uuid::new_v4().to_string(),
            volumes: vec![],
        };
        let pg_volume = format!("{}-data", value.postgres);
        let s3_volume = format!("{}-data", value.s3);
        value.volumes = vec![pg_volume.clone(), s3_volume.clone()];
        docker(&[
            "run",
            "--rm",
            "-d",
            "--name",
            &value.postgres,
            "--memory",
            "512m",
            "--cpus",
            "1",
            "-e",
            &format!("POSTGRES_PASSWORD={}", value.password),
            "--publish",
            "127.0.0.1::5432",
            "--mount",
            &format!("type=volume,source={pg_volume},target=/var/lib/postgresql"),
            "postgres:18.4",
        ]);
        docker(&[
            "run",
            "--rm",
            "-d",
            "--name",
            &value.s3,
            "--memory",
            "768m",
            "--cpus",
            "1",
            "-e",
            &format!("AWS_ACCESS_KEY_ID={}", value.access),
            "-e",
            &format!("AWS_SECRET_ACCESS_KEY={}", value.secret),
            "-e",
            "S3_BUCKET=sessiondb-test",
            "--publish",
            "127.0.0.1::8333",
            "--mount",
            &format!("type=volume,source={s3_volume},target=/data"),
            "chrislusf/seaweedfs:4.54",
        ]);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(pool) = sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .acquire_timeout(Duration::from_secs(1))
                .connect(&value.database_url("postgres"))
                .await
            {
                pool.close().await;
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "PostgreSQL did not become ready"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let sdk = value.s3_client();
        loop {
            if sdk
                .head_bucket()
                .bucket("sessiondb-test")
                .send()
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "S3 did not become ready"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        value
    }
    pub fn database_url(&self, name: &str) -> String {
        format!(
            "postgres://postgres:{}@127.0.0.1:{}/{name}",
            self.password,
            port(&self.postgres, "5432/tcp")
        )
    }
    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", port(&self.s3, "8333/tcp"))
    }
    pub fn s3_client(&self) -> aws_sdk_s3::Client {
        use aws_sdk_s3::config::*;
        aws_sdk_s3::Client::from_conf(
            Builder::new()
                .behavior_version(BehaviorVersion::v2026_11_10())
                .region(Region::new("us-east-1"))
                .endpoint_url(self.endpoint())
                .force_path_style(true)
                .credentials_provider(Credentials::new(
                    &self.access,
                    &self.secret,
                    None,
                    None,
                    "synthetic-integration",
                ))
                .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
                .retry_config(retry::RetryConfig::standard().with_max_attempts(1))
                .timeout_config(
                    timeout::TimeoutConfig::builder()
                        .operation_timeout(Duration::from_secs(5))
                        .build(),
                )
                .build(),
        )
    }
    pub fn backup_restore(&self) {
        let dump = docker(&[
            "exec",
            &self.postgres,
            "pg_dump",
            "-U",
            "postgres",
            "-Fc",
            "postgres",
        ])
        .stdout;
        docker(&[
            "exec",
            &self.postgres,
            "createdb",
            "-U",
            "postgres",
            "restored",
        ]);
        let mut child = Command::new("docker")
            .args([
                "exec",
                "-i",
                &self.postgres,
                "pg_restore",
                "-U",
                "postgres",
                "-d",
                "restored",
                "--exit-on-error",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&dump).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "restore failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
impl Drop for Services {
    fn drop(&mut self) {
        for name in [&self.s3, &self.postgres] {
            let _ = Command::new("docker")
                .args(["rm", "-f", "-v", name])
                .output();
        }
        for name in &self.volumes {
            let _ = Command::new("docker").args(["volume", "rm", name]).output();
        }
    }
}

pub struct Server {
    pub url: String,
    pub config: PathBuf,
    pub log: PathBuf,
    pub api_key: String,
    child: Option<Child>,
}
impl Server {
    pub async fn start(root: &Path, services: &Services) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let mut value = Self {
            url: format!("http://{address}"),
            config: root.join("server.toml"),
            log: root.join("server.log"),
            api_key: format!("test-{}", Uuid::new_v4()),
            child: None,
        };
        value.write_config(services);
        let output = value
            .command(services, "postgres")
            .arg("migrate")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "migrations failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        value.restart(services, "postgres").await;
        value
    }
    pub fn write_config(&self, services: &Services) {
        fs::write(
            &self.config,
            format!(
                r#"listen = "{}"
public_url = "{}"
[objects]
provider = "s3"
endpoint = "{}"
bucket = "sessiondb-test"
region = "us-east-1"
path_style = true
"#,
                self.url.trim_start_matches("http://"),
                self.url,
                services.endpoint()
            ),
        )
        .unwrap();
    }
    pub fn command(&self, services: &Services, database: &str) -> Command {
        let executable =
            Path::new(env!("CARGO_BIN_EXE_sessiondb")).with_file_name("sessiondb-server");
        assert!(
            executable.exists(),
            "build the workspace binaries before running ignored integration tests"
        );
        let mut command = Command::new(executable);
        command
            .arg("--config")
            .arg(&self.config)
            .env("SESSIONDB_DATABASE_URL", services.database_url(database))
            .env("SESSIONDB_S3_ACCESS_KEY_ID", &services.access)
            .env("SESSIONDB_S3_SECRET_ACCESS_KEY", &services.secret)
            .env("SESSIONDB_API_KEY", &self.api_key)
            .env("AWS_EC2_METADATA_DISABLED", "true");
        command
    }
    pub async fn restart(&mut self, services: &Services, database: &str) {
        self.stop();
        let log = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log)
            .unwrap();
        self.child = Some(
            self.command(services, database)
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        let http = reqwest::Client::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                panic!(
                    "server exited {status}: {}",
                    fs::read_to_string(&self.log).unwrap()
                );
            }
            if http
                .get(format!("{}/readyz", self.url))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "server readiness timed out: {}",
                fs::read_to_string(&self.log).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn cli_with_saved_server(server: &Server, root: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_sessiondb"));
    command
        .arg("--config")
        .arg(root.join("client.toml"))
        .arg("--state-dir")
        .arg(root.join("state"))
        .env_remove("SESSIONDB_URL")
        .env("SESSIONDB_API_KEY", &server.api_key);
    command.kill_on_drop(true);
    command
}
pub fn cli_command(server: &Server, root: &Path, args: &[&str]) -> tokio::process::Command {
    let mut command = cli_with_saved_server(server, root);
    command.arg("--server").arg(&server.url).args(args);
    command
}
pub async fn cli(server: &Server, root: &Path, args: &[&str]) -> std::process::Output {
    cli_command(server, root, args).output().await.unwrap()
}
