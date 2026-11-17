use sessiondb_server::config::{Auth, Config, require_secure_url};
use std::process::Command;

#[test]
fn auth_defaults_to_the_instance_key_environment_variable() {
    let auth: Auth = toml::from_str("").unwrap();
    assert_eq!(auth.api_key_env, "SESSIONDB_API_KEY");
    assert!(toml::from_str::<Auth>("issuer = 'https://identity.example.com'").is_err());
}

#[test]
fn configuration_requires_a_nonempty_instance_key() {
    const CHILD: &str = "SESSIONDB_CONFIGURATION_TEST_CHILD";
    const KEY: &str = "SESSIONDB_CONFIGURATION_TEST_KEY";
    if let Ok(case) = std::env::var(CHILD) {
        let mut config: Config =
            toml::from_str(include_str!("../../../docs/examples/server.toml")).unwrap();
        config.auth.api_key_env = KEY.into();
        let result = config.validate();
        if case == "valid" {
            result.unwrap();
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("API key"), "{error}");
        }
        return;
    }
    for case in ["missing", "empty", "valid"] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "configuration_requires_a_nonempty_instance_key"])
            .env(CHILD, case)
            .env_remove(KEY);
        if case != "missing" {
            child.env(
                KEY,
                if case == "valid" {
                    "test-instance-secret"
                } else {
                    ""
                },
            );
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "configuration case {case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn plain_http_is_limited_to_loopback_development() {
    assert!(require_secure_url(&"https://sessiondb.example.com".parse().unwrap(), false).is_ok());
    assert!(require_secure_url(&"http://127.0.0.1:8180".parse().unwrap(), true).is_ok());
    assert!(require_secure_url(&"http://127.0.0.1:8180".parse().unwrap(), false).is_err());
    assert!(require_secure_url(&"http://sessiondb.example.com".parse().unwrap(), true).is_err());
}
