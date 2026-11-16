use sessiondb_server::config::{Config, require_secure_url};

#[test]
fn oidc_issuer_spelling_is_preserved_exactly() {
    let example = include_str!("../../../docs/examples/server.toml");
    let text = example.replace(
        "https://identity.example.com/realms/acme",
        "https://identity.example.com",
    );
    let config: Config = toml::from_str(&text).unwrap();
    config.validate().unwrap();
    assert_eq!(config.auth.issuer, "https://identity.example.com");
}
#[test]
fn plain_http_identity_is_limited_to_explicit_loopback_development() {
    assert!(require_secure_url(&"https://identity.example.com".parse().unwrap(), false).is_ok());
    assert!(require_secure_url(&"http://127.0.0.1:8180".parse().unwrap(), true).is_ok());
    assert!(require_secure_url(&"http://127.0.0.1:8180".parse().unwrap(), false).is_err());
    assert!(require_secure_url(&"http://identity.example.com".parse().unwrap(), true).is_err());
}
