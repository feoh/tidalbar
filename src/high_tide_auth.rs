//! Optional High Tide-compatible playback login. The client identity and
//! refresh implementation stay in the user's installed `tidalapi` package;
//! tidalbar never distributes another application's client credentials.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keyring::Entry;
use oauth2::{CsrfToken, PkceCodeChallenge};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

const SERVICE: &str = "tidalbar";
const USER: &str = "high-tide-playback";
const AUTHORIZE_URL: &str = "https://login.tidal.com/authorize";
const TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";

// Only public, non-secret configuration is returned by this helper. No client
// identifier or client secret is checked into this repository.
const CONFIG_HELPER: &str = "import json,tidalapi; c=tidalapi.Config(); print(json.dumps({'client_id':c.client_id_pkce,'redirect_uri':c.pkce_uri_redirect}))";
// The refresh token travels over stdin, never argv; stdout is captured by Rust,
// not forwarded to the terminal. The package owns its own refresh credentials.
const REFRESH_HELPER: &str = r#"import sys,json,datetime,tidalapi
r=json.load(sys.stdin)
s=tidalapi.Session()
s.is_pkce=True
if not s.token_refresh(r['refresh_token']):
    sys.exit(1)
seconds=max(0,int((s.expiry_time-datetime.datetime.utcnow()).total_seconds()))
print(json.dumps({'access_token':s.access_token,'expires_in':seconds}))
"#;

#[derive(Debug, Error)]
pub enum PlaybackAuthError {
    #[error("install Python's tidalapi package to use High Tide-compatible playback login")]
    MissingDependency,
    #[error("tidalapi playback authentication failed; log in again")]
    Authentication,
    #[error("invalid High Tide-compatible authentication response")]
    InvalidResponse,
    #[error("the redirect must be the URL shown by the TIDAL login page")]
    InvalidRedirect,
    #[error("OS credential store is unavailable: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("could not decode stored playback authorization")]
    StoredToken,
    #[error("network request failed: {0}")]
    Network(#[from] reqwest::Error),
}

#[derive(Clone, Deserialize, Serialize)]
pub struct PlaybackTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_unix: u64,
}

impl PlaybackTokens {
    pub fn expires_soon(&self) -> bool {
        self.expires_at_unix <= now_unix().saturating_add(60)
    }
}

pub struct PlaybackTokenStore;

impl PlaybackTokenStore {
    pub fn load(&self) -> Result<Option<PlaybackTokens>, PlaybackAuthError> {
        let entry = Entry::new(SERVICE, USER)?;
        match entry.get_password() {
            Ok(value) => serde_json::from_str(&value)
                .map(Some)
                .map_err(|_| PlaybackAuthError::StoredToken),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, tokens: &PlaybackTokens) -> Result<(), PlaybackAuthError> {
        let entry = Entry::new(SERVICE, USER)?;
        let encoded = serde_json::to_string(tokens).map_err(|_| PlaybackAuthError::StoredToken)?;
        entry.set_password(&encoded)?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), PlaybackAuthError> {
        let entry = Entry::new(SERVICE, USER)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Deserialize)]
struct ClientConfig {
    client_id: String,
    redirect_uri: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    expires_in: u64,
}

fn python() -> std::ffi::OsString {
    std::env::var_os("TIDALBAR_PYTHON")
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        })
}

fn client_config() -> Result<ClientConfig, PlaybackAuthError> {
    let output = Command::new(python())
        .arg("-c")
        .arg(CONFIG_HELPER)
        .stderr(Stdio::null())
        .output()
        .map_err(|_| PlaybackAuthError::MissingDependency)?;
    if !output.status.success() {
        return Err(PlaybackAuthError::MissingDependency);
    }
    let config: ClientConfig =
        serde_json::from_slice(&output.stdout).map_err(|_| PlaybackAuthError::InvalidResponse)?;
    let redirect =
        Url::parse(&config.redirect_uri).map_err(|_| PlaybackAuthError::InvalidResponse)?;
    if config.client_id.is_empty()
        || redirect.scheme() != "https"
        || redirect.host_str() != Some("tidal.com")
    {
        return Err(PlaybackAuthError::InvalidResponse);
    }
    Ok(config)
}

pub async fn login() -> Result<PlaybackTokens, PlaybackAuthError> {
    let config = client_config()?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let state = CsrfToken::new_random();
    // tidalapi uses a 64-bit hexadecimal client_unique_key.
    let unique_key = state.secret().as_bytes()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut url = Url::parse(AUTHORIZE_URL).map_err(|_| PlaybackAuthError::InvalidResponse)?;
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("redirect_uri", config.redirect_uri.as_str()),
        ("client_id", config.client_id.as_str()),
        ("lang", "EN"),
        ("appMode", "android"),
        ("client_unique_key", unique_key.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("restrict_signup", "true"),
        ("state", state.secret()),
    ]);
    println!("Open this TIDAL login URL in your browser:\n{url}");
    let _ = webbrowser::open(url.as_str());
    println!("Paste the final redirected URL here (not into chat):");
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|_| PlaybackAuthError::InvalidRedirect)?;
    let code = authorization_code(input.trim(), &config.redirect_uri, state.secret())?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = http
        .post(TOKEN_URL)
        .form(&[
            ("code", code.as_str()),
            ("client_id", config.client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("scope", "r_usr+w_usr+w_sub"),
            ("code_verifier", verifier.secret()),
            ("client_unique_key", unique_key.as_str()),
        ])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(PlaybackAuthError::Authentication);
    }
    let tokens: TokenResponse = response
        .json()
        .await
        .map_err(|_| PlaybackAuthError::InvalidResponse)?;
    Ok(PlaybackTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at_unix: now_unix().saturating_add(tokens.expires_in),
    })
}

fn authorization_code(
    input: &str,
    redirect_uri: &str,
    expected_state: &str,
) -> Result<String, PlaybackAuthError> {
    let redirect = Url::parse(input).map_err(|_| PlaybackAuthError::InvalidRedirect)?;
    let expected = Url::parse(redirect_uri).map_err(|_| PlaybackAuthError::InvalidResponse)?;
    if redirect.scheme() != expected.scheme()
        || redirect.host_str() != expected.host_str()
        || redirect.port() != expected.port()
        || redirect.path() != expected.path()
        || !redirect.username().is_empty()
        || redirect.password().is_some()
        || redirect.fragment().is_some()
    {
        return Err(PlaybackAuthError::InvalidRedirect);
    }
    let states: Vec<_> = redirect
        .query_pairs()
        .filter(|(key, _)| key == "state")
        .map(|(_, value)| value)
        .collect();
    // Some redirects omit state. PKCE still binds the code to this login's
    // verifier, but if state is returned it must match exactly once.
    if states.len() > 1 || states.first().is_some_and(|value| value != expected_state) {
        return Err(PlaybackAuthError::InvalidRedirect);
    }
    let codes: Vec<_> = redirect
        .query_pairs()
        .filter(|(key, _)| key == "code")
        .map(|(_, value)| value)
        .collect();
    if codes.len() != 1 || codes[0].is_empty() {
        return Err(PlaybackAuthError::InvalidRedirect);
    }
    Ok(codes[0].to_string())
}

pub fn refresh(tokens: &PlaybackTokens) -> Result<PlaybackTokens, PlaybackAuthError> {
    let mut child = Command::new(python())
        .arg("-c")
        .arg(REFRESH_HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| PlaybackAuthError::MissingDependency)?;
    let input = serde_json::json!({"refresh_token": tokens.refresh_token});
    let encoded = serde_json::to_vec(&input).map_err(|_| PlaybackAuthError::InvalidResponse)?;
    child
        .stdin
        .take()
        .ok_or(PlaybackAuthError::Authentication)?
        .write_all(&encoded)
        .map_err(|_| PlaybackAuthError::Authentication)?;
    let output = child
        .wait_with_output()
        .map_err(|_| PlaybackAuthError::Authentication)?;
    if !output.status.success() {
        return Err(PlaybackAuthError::Authentication);
    }
    let refreshed: RefreshResponse =
        serde_json::from_slice(&output.stdout).map_err(|_| PlaybackAuthError::InvalidResponse)?;
    Ok(PlaybackTokens {
        access_token: refreshed.access_token,
        refresh_token: tokens.refresh_token.clone(),
        expires_at_unix: now_unix().saturating_add(refreshed.expires_in),
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_tokens_are_separate_from_catalog_tokens() {
        assert_ne!(USER, "tidal-oauth");
    }

    #[test]
    fn playback_redirect_rejects_wrong_origin_or_ambiguous_parameters() {
        let expected = "https://tidal.com/android/login/auth";
        assert_eq!(
            authorization_code(
                "https://tidal.com/android/login/auth?code=example&state=expected",
                expected,
                "expected"
            )
            .expect("valid redirect"),
            "example"
        );
        for url in [
            "https://evil.test/android/login/auth?code=example&state=expected",
            "https://tidal.com:444/android/login/auth?code=example&state=expected",
            "https://tidal.com/android/login/auth?code=example&state=wrong",
            "https://tidal.com/android/login/auth?code=a&code=b&state=expected",
            "https://tidal.com/android/login/auth?code=a&state=expected&state=expected",
        ] {
            assert!(authorization_code(url, expected, "expected").is_err());
        }
    }

    #[test]
    fn expired_playback_token_is_detected() {
        let token = PlaybackTokens {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at_unix: 0,
        };
        assert!(token.expires_soon());
    }
}
