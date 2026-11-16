//! Demonstrate browser login and API access without persisting credentials.
use sessiondb_client::{
    Client,
    auth::{LoginFlow, refresh},
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run -p sessiondb-client --example login -- SERVER_URL")?;
    let anonymous = Client::new(&server, None, Duration::from_secs(30))?;
    let configuration = anonymous.auth_config().await?;
    let flow = LoginFlow::start(&configuration).await?;
    println!("Open this URL in your browser:\n{}", flow.authorization_url);
    let tokens = flow.finish().await?;
    let authenticated = Client::new(
        &server,
        Some(tokens.access_token.clone()),
        Duration::from_secs(30),
    )?;
    let account = authenticated.register_login().await?;
    println!(
        "Signed in as {} ({} organization memberships)",
        account.user.display_name,
        account.memberships.len()
    );
    if tokens.refresh_token.is_some() {
        let _ = refresh(&configuration, &tokens).await?;
        println!("Refresh succeeded.");
    }
    Ok(())
}
