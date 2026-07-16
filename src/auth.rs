use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keyring::Entry;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const KEYRING_SERVICE: &str = "tidalbar";
const KEYRING_USER: &str = "tidal-oauth";
const AUTHORIZE_URL: &str = "https://login.tidal.com/authorize";
const TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

pub const USER_SCOPES: [&str; 6] = [
    "collection.read",
    "playback",
    "playlists.read",
    "recommendations.read",
    "search.read",
    "user.read",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoredTokens {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix: Option<u64>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum TokenStoreError {
    #[error("OS credential store is unavailable: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("stored TIDAL credentials are invalid: {0}")]
    Decode(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum LoginError {
    #[error("invalid OAuth configuration: {0}")]
    Configuration(String),
    #[error("redirect URI must use http with a localhost or loopback IP host")]
    UnsupportedRedirect,
    #[error("could not listen for the OAuth redirect at {address}: {source}")]
    Listen {
        address: String,
        source: std::io::Error,
    },
    #[error("timed out waiting for TIDAL login")]
    Timeout,
    #[error("could not read the OAuth redirect: {0}")]
    CallbackIo(#[source] std::io::Error),
    #[error("invalid OAuth redirect: {0}")]
    InvalidCallback(String),
    #[error("TIDAL login was denied: {0}")]
    Denied(String),
    #[error("OAuth state did not match; login was discarded")]
    StateMismatch,
    #[error("could not open the system browser: {0}")]
    Browser(#[source] std::io::Error),
    #[error("TIDAL token exchange failed: {0}")]
    TokenExchange(String),
}

#[derive(Debug, Default)]
pub struct TokenStore;

impl TokenStore {
    pub fn load(&self) -> Result<Option<StoredTokens>, TokenStoreError> {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
        match entry.get_password() {
            Ok(serialized) => Ok(Some(serde_json::from_str(&serialized)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, tokens: &StoredTokens) -> Result<(), TokenStoreError> {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
        entry.set_password(&serde_json::to_string(tokens)?)?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), TokenStoreError> {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl StoredTokens {
    pub fn expires_soon(&self) -> bool {
        self.expires_at_unix
            .is_some_and(|expires| expires <= now_unix().saturating_add(60))
    }
}

pub async fn refresh(tokens: &StoredTokens) -> Result<StoredTokens, LoginError> {
    let refresh_token = tokens
        .refresh_token
        .as_ref()
        .ok_or_else(|| LoginError::TokenExchange("no refresh token is available".to_owned()))?;
    let client = BasicClient::new(ClientId::new(tokens.client_id.clone()))
        .set_token_uri(
            TokenUrl::new(TOKEN_URL.to_owned())
                .map_err(|error| LoginError::Configuration(error.to_string()))?,
        )
        .set_auth_type(AuthType::RequestBody);
    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| LoginError::Configuration(error.to_string()))?;
    let response = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.clone()))
        .request_async(&http_client)
        .await
        .map_err(|error| LoginError::TokenExchange(error.to_string()))?;
    let scopes = response.scopes().map_or_else(
        || tokens.scopes.clone(),
        |scopes| scopes.iter().map(|scope| scope.to_string()).collect(),
    );
    Ok(StoredTokens {
        client_id: tokens.client_id.clone(),
        access_token: response.access_token().secret().to_owned(),
        refresh_token: response.refresh_token().map_or_else(
            || tokens.refresh_token.clone(),
            |token| Some(token.secret().to_owned()),
        ),
        expires_at_unix: response
            .expires_in()
            .map(|duration| now_unix() + duration.as_secs()),
        scopes,
    })
}

pub async fn login(client_id: &str, redirect_uri: &str) -> Result<StoredTokens, LoginError> {
    let redirect =
        Url::parse(redirect_uri).map_err(|error| LoginError::Configuration(error.to_string()))?;
    let host = redirect.host_str().ok_or(LoginError::UnsupportedRedirect)?;
    let is_loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if redirect.scheme() != "http" || !is_loopback {
        return Err(LoginError::UnsupportedRedirect);
    }
    let port = redirect
        .port_or_known_default()
        .ok_or(LoginError::UnsupportedRedirect)?;
    let address = format!("{host}:{port}");
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|source| LoginError::Listen {
            address: address.clone(),
            source,
        })?;

    let client = BasicClient::new(ClientId::new(client_id.to_owned()))
        .set_auth_uri(
            AuthUrl::new(AUTHORIZE_URL.to_owned())
                .map_err(|error| LoginError::Configuration(error.to_string()))?,
        )
        .set_token_uri(
            TokenUrl::new(TOKEN_URL.to_owned())
                .map_err(|error| LoginError::Configuration(error.to_string()))?,
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_owned())
                .map_err(|error| LoginError::Configuration(error.to_string()))?,
        )
        .set_auth_type(AuthType::RequestBody);

    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut request = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(challenge);
    for scope in USER_SCOPES {
        request = request.add_scope(Scope::new(scope.to_owned()));
    }
    let (authorize_url, expected_state) = request.url();

    println!("Opening TIDAL login in your browser…");
    println!("If it does not open, visit:\n{authorize_url}\n");
    webbrowser::open(authorize_url.as_str()).map_err(LoginError::Browser)?;

    let callback = receive_callback(listener, &redirect).await?;
    let parameters = callback
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    if let Some(error) = parameters.get("error") {
        let detail = parameters
            .get("error_description")
            .map_or_else(|| error.to_string(), ToString::to_string);
        return Err(LoginError::Denied(detail));
    }
    let state = parameters
        .get("state")
        .ok_or_else(|| LoginError::InvalidCallback("missing state".to_owned()))?;
    if state.as_ref() != expected_state.secret() {
        return Err(LoginError::StateMismatch);
    }
    let code = parameters
        .get("code")
        .ok_or_else(|| LoginError::InvalidCallback("missing authorization code".to_owned()))?;

    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| LoginError::Configuration(error.to_string()))?;
    let response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(verifier)
        .request_async(&http_client)
        .await
        .map_err(|error| LoginError::TokenExchange(error.to_string()))?;

    let expires_at_unix = response
        .expires_in()
        .map(|duration| now_unix() + duration.as_secs());
    let scopes = response.scopes().map_or_else(
        || USER_SCOPES.iter().map(ToString::to_string).collect(),
        |scopes| scopes.iter().map(|scope| scope.to_string()).collect(),
    );

    Ok(StoredTokens {
        client_id: client_id.to_owned(),
        access_token: response.access_token().secret().to_owned(),
        refresh_token: response
            .refresh_token()
            .map(|token| token.secret().to_owned()),
        expires_at_unix,
        scopes,
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn receive_callback(listener: TcpListener, redirect: &Url) -> Result<Url, LoginError> {
    let (mut stream, _) = tokio::time::timeout(LOGIN_TIMEOUT, listener.accept())
        .await
        .map_err(|_| LoginError::Timeout)?
        .map_err(LoginError::CallbackIo)?;
    let mut buffer = vec![0_u8; 16 * 1024];
    let count = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer))
        .await
        .map_err(|_| LoginError::Timeout)?
        .map_err(LoginError::CallbackIo)?;
    let request = String::from_utf8_lossy(&buffer[..count]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| LoginError::InvalidCallback("malformed HTTP request".to_owned()))?;
    let callback = redirect
        .join(target)
        .map_err(|error| LoginError::InvalidCallback(error.to_string()))?;

    let valid_path = callback.path() == redirect.path();
    let message = if valid_path {
        "TIDAL authorization received. You may close this tab and return to tidalbar."
    } else {
        "Invalid tidalbar authorization callback."
    };
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        if valid_path {
            "200 OK"
        } else {
            "404 Not Found"
        },
        message.len(),
        message
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(LoginError::CallbackIo)?;
    if !valid_path {
        return Err(LoginError::InvalidCallback(
            "redirect path did not match configured URI".to_owned(),
        ));
    }
    Ok(callback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_tokens_do_not_require_a_client_secret() {
        let tokens = StoredTokens {
            client_id: "public-client".to_owned(),
            access_token: "access".to_owned(),
            refresh_token: Some("refresh".to_owned()),
            expires_at_unix: Some(42),
            scopes: USER_SCOPES.iter().map(ToString::to_string).collect(),
        };

        let json = serde_json::to_string(&tokens).expect("tokens serialize");

        assert!(!json.contains("client_secret"));
    }

    #[tokio::test]
    async fn non_loopback_redirects_are_rejected_before_opening_a_browser() {
        let result = login("client", "https://example.test/callback").await;

        assert!(matches!(result, Err(LoginError::UnsupportedRedirect)));
    }
}
