use axum::{
    Json, Router,
    extract::{Form, Query, State},
    response::Redirect,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::{RsaPrivateKey, pkcs8::EncodePrivateKey, traits::PublicKeyParts};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::{net::TcpListener, sync::Mutex};
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

pub struct Issuer {
    pub url: String,
    pub state: Arc<IssuerState>,
    task: tokio::task::JoinHandle<()>,
}
pub struct IssuerState {
    pub url: String,
    key: EncodingKey,
    jwk: Value,
    codes: Mutex<HashMap<String, (String, String)>>,
    refresh: Mutex<HashMap<String, String>>,
    pub bad_refresh_subject: std::sync::atomic::AtomicBool,
}
impl Issuer {
    pub async fn start() -> Self {
        let key = RsaPrivateKey::new(&mut rand08::rngs::OsRng, 2048).unwrap();
        let pem = key.to_pkcs8_pem(Default::default()).unwrap();
        let jwk = json!({"kty":"RSA","kid":"test-key","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())});
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(IssuerState {
            url: url.clone(),
            key: EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap(),
            jwk,
            codes: Mutex::new(HashMap::new()),
            refresh: Mutex::new(HashMap::new()),
            bad_refresh_subject: std::sync::atomic::AtomicBool::new(false),
        });
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .with_state(state.clone());
        let task = tokio::spawn(async {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, state, task }
    }
    pub fn token(&self, subject: &str) -> String {
        self.claims(
            json!({"sub":subject,"email":format!("{subject}@example.com"),"email_verified":true}),
        )
    }
    pub fn claims(&self, overrides: Value) -> String {
        let mut claims = json!({"iss":self.url,"aud":"sessiondb-api","sub":"owner","exp":chrono::Utc::now().timestamp()+3600,"iat":chrono::Utc::now().timestamp(),"name":"Synthetic account","email":"owner@example.com","email_verified":true});
        for (key, value) in overrides.as_object().unwrap() {
            if value.is_null() {
                claims.as_object_mut().unwrap().remove(key);
            } else {
                claims[key] = value.clone();
            }
        }
        sign(&self.state, claims)
    }
}
impl Drop for Issuer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn sign(state: &IssuerState, claims: Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".into());
    jsonwebtoken::encode(&header, &claims, &state.key).unwrap()
}
async fn discovery(State(s): State<Arc<IssuerState>>) -> Json<Value> {
    Json(
        json!({"issuer":s.url,"authorization_endpoint":format!("{}/authorize",s.url),"token_endpoint":format!("{}/token",s.url),"jwks_uri":format!("{}/jwks",s.url),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"],"token_endpoint_auth_methods_supported":["none"],"scopes_supported":["openid","profile","email"]}),
    )
}
async fn jwks(State(s): State<Arc<IssuerState>>) -> Json<Value> {
    Json(json!({"keys":[s.jwk]}))
}
async fn authorize(
    State(s): State<Arc<IssuerState>>,
    Query(p): Query<HashMap<String, String>>,
) -> Redirect {
    assert_eq!(
        p.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    let code = Uuid::new_v4().to_string();
    s.codes.lock().await.insert(
        code.clone(),
        (p["code_challenge"].clone(), p["nonce"].clone()),
    );
    let mut url = url::Url::parse(&p["redirect_uri"]).unwrap();
    url.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &p["state"]);
    Redirect::temporary(url.as_str())
}
async fn token(
    State(s): State<Arc<IssuerState>>,
    Form(p): Form<HashMap<String, String>>,
) -> (axum::http::StatusCode, Json<Value>) {
    let refreshing = p.get("grant_type").map(String::as_str) == Some("refresh_token");
    let nonce = if refreshing {
        let previous = s
            .refresh
            .lock()
            .await
            .remove(p.get("refresh_token").map(String::as_str).unwrap_or(""));
        if previous.is_none() {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid_grant"})),
            );
        }
        previous
    } else {
        let Some((challenge, nonce)) = s
            .codes
            .lock()
            .await
            .remove(p.get("code").map(String::as_str).unwrap_or(""))
        else {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid_grant"})),
            );
        };
        if URL_SAFE_NO_PAD.encode(Sha256::digest(
            p.get("code_verifier").map(String::as_str).unwrap_or(""),
        )) != challenge
        {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid_grant"})),
            );
        }
        Some(nonce)
    };
    let now = chrono::Utc::now().timestamp();
    let access = sign(
        &s,
        json!({"iss":s.url,"aud":"sessiondb-api","sub":"owner","exp":now+60,"iat":now,"name":"Owner","email":"owner@example.com","email_verified":true}),
    );
    let refresh = Uuid::new_v4().to_string();
    s.refresh
        .lock()
        .await
        .insert(refresh.clone(), nonce.clone().unwrap_or_default());
    let mut response = json!({"access_token":access,"token_type":"Bearer","expires_in":60,"refresh_token":refresh});
    if let Some(nonce) = nonce {
        let subject = if refreshing
            && s.bad_refresh_subject
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            "wrong-user"
        } else {
            "owner"
        };
        response["id_token"] = json!(sign(
            &s,
            json!({"iss":s.url,"aud":"sessiondb-cli","sub":subject,"exp":now+60,"iat":now,"nonce":nonce})
        ));
    }
    (axum::http::StatusCode::OK, Json(response))
}

pub struct Server {
    pub url: String,
    pub config: PathBuf,
    pub log: PathBuf,
    child: Option<Child>,
}
impl Server {
    pub async fn start(root: &Path, services: &Services, issuer: &Issuer) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let mut value = Self {
            url: format!("http://{address}"),
            config: root.join("server.toml"),
            log: root.join("server.log"),
            child: None,
        };
        value.write_config(services, issuer);
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
    pub fn write_config(&self, services: &Services, issuer: &Issuer) {
        fs::write(
            &self.config,
            format!(
                r#"listen = "{}"
public_url = "{}"
[database]
url_env = "SESSIONDB_TEST_DATABASE"
[objects]
provider = "s3"
endpoint = "{}"
bucket = "sessiondb-test"
region = "us-east-1"
path_style = true
access_key_env = "SESSIONDB_TEST_ACCESS"
secret_key_env = "SESSIONDB_TEST_SECRET"
[auth]
issuer = "{}"
audience = "sessiondb-api"
cli_client_id = "sessiondb-cli"
allow_insecure_http = true
[bootstrap]
organization_slug = "test"
organization_name = "Test"
owner_issuer = "{}"
owner_subject = "owner"
"#,
                self.url.trim_start_matches("http://"),
                self.url,
                services.endpoint(),
                issuer.url,
                issuer.url
            ),
        )
        .unwrap();
    }
    fn command(&self, services: &Services, database: &str) -> Command {
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
            .env("SESSIONDB_TEST_DATABASE", services.database_url(database))
            .env("SESSIONDB_TEST_ACCESS", &services.access)
            .env("SESSIONDB_TEST_SECRET", &services.secret)
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

pub fn cli_command(
    server: &Server,
    issuer: &Issuer,
    root: &Path,
    args: &[&str],
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_sessiondb"));
    command
        .arg("--config")
        .arg(root.join("client.toml"))
        .arg("--state-dir")
        .arg(root.join("state"))
        .arg("--server")
        .arg(&server.url)
        .arg("--organization")
        .arg("test")
        .env("SESSIONDB_TOKEN", issuer.token("owner"))
        .args(args);
    command.kill_on_drop(true);
    command
}
pub async fn cli(
    server: &Server,
    issuer: &Issuer,
    root: &Path,
    args: &[&str],
) -> std::process::Output {
    cli_command(server, issuer, root, args)
        .output()
        .await
        .unwrap()
}
