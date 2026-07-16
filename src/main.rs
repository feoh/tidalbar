use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use crossterm::event::{self, Event};
use tidalbar::app::{Action, App, Screen};
use tidalbar::artwork::ArtworkState;
use tidalbar::auth::{TokenStore, login, refresh};
use tidalbar::config::{AppConfig, config_path, config_path_display};
use tidalbar::models::{MediaItem, MediaKind, Shelf};
use tidalbar::playback::{AudioEngine, MediaResolver, MpvEngine, PreviewResolver};
use tidalbar::tidal::TidalClient;
use tidalbar::ui;

#[derive(Debug, Parser)]
#[command(
    name = "tidalbar",
    version,
    about = "A keyboard-driven terminal client for TIDAL"
)]
struct Cli {
    /// Render cover art as text instead of querying terminal image capabilities.
    #[arg(long)]
    no_images: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Test credentials and official API access without displaying user data.
    Doctor,
    /// Inspect or update non-secret local configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Inspect or clear securely stored user authorization.
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Print the platform-specific configuration path.
    Path,
    /// Save the public OAuth client ID (never a client secret).
    SetClientId { client_id: String },
    /// Save the exact loopback redirect URI registered in the TIDAL dashboard.
    SetRedirectUri { redirect_uri: String },
}

#[derive(Debug, Subcommand)]
enum AuthAction {
    /// Log in through the system browser using OAuth Authorization Code + PKCE.
    Login,
    /// Report whether an OAuth token is present in the OS credential store.
    Status,
    /// Delete the locally stored OAuth token.
    Logout,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(command) = cli.command {
        return run_command(command).await;
    }

    let config = AppConfig::load().context("could not load tidalbar configuration")?;
    let mut tokens = TokenStore.load().ok().flatten();
    let mut token_warning = None;
    if let Some(stored) = tokens.as_ref().filter(|stored| {
        stored.expires_soon() && config.client_id.as_deref() == Some(stored.client_id.as_str())
    }) {
        match refresh(stored).await {
            Ok(updated) => {
                TokenStore.save(&updated)?;
                tokens = Some(updated);
            }
            Err(error) => token_warning = Some(format!("Token refresh failed: {error}")),
        }
    }
    let authenticated = tokens
        .as_ref()
        .is_some_and(|tokens| config.client_id.as_deref() == Some(tokens.client_id.as_str()));
    let tidal = tokens
        .filter(|tokens| config.client_id.as_deref() == Some(tokens.client_id.as_str()))
        .map(|tokens| TidalClient::new(tokens.access_token));
    let mut artwork = if cli.no_images {
        Some(ArtworkState::halfblocks())
    } else {
        Some(ArtworkState::detect())
    };
    let mut app = App::new(authenticated);
    if config.client_id.is_none() {
        app.status = "Set a public TIDAL client ID in config.toml to enable login".to_owned();
    } else if let Some(warning) = token_warning {
        app.status = warning;
    } else if let Some(client) = tidal.as_ref() {
        match load_screen(client, Screen::ForYou).await {
            Ok(shelves) => app.replace_shelves(shelves),
            Err(error) => app.playback_failed(format!("Could not load For You: {error}")),
        }
    }
    update_artwork(tidal.as_ref(), app.selected(), artwork.as_mut()).await;

    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal, &mut app, artwork.as_mut(), tidal.as_ref()).await;
    ratatui::restore();
    result
}

async fn run_command(command: Command) -> Result<()> {
    match command {
        Command::Doctor => run_doctor().await?,
        Command::Config {
            action: ConfigAction::Path,
        } => {
            println!("{}", config_path_display(&config_path()?));
        }
        Command::Config {
            action: ConfigAction::SetClientId { client_id },
        } => {
            let mut config = AppConfig::load()?;
            config.client_id = Some(client_id);
            config.save()?;
            println!("Public TIDAL client ID saved; no client secret was stored");
        }
        Command::Config {
            action: ConfigAction::SetRedirectUri { redirect_uri },
        } => {
            let mut config = AppConfig::load()?;
            config.redirect_uri = Some(redirect_uri);
            config.save()?;
            println!("OAuth redirect URI saved");
        }
        Command::Auth {
            action: AuthAction::Login,
        } => {
            let config = AppConfig::load()?;
            let client_id = config.client_id.context(
                "set the public client ID with `tidalbar config set-client-id <CLIENT_ID>`",
            )?;
            let redirect_uri = config.redirect_uri.context(
                "register a loopback redirect in TIDAL, then run `tidalbar config set-redirect-uri <URI>`",
            )?;
            let tokens = login(&client_id, &redirect_uri).await?;
            TokenStore.save(&tokens)?;
            println!("TIDAL authorization stored securely in the OS credential store");
        }
        Command::Auth {
            action: AuthAction::Status,
        } => match TokenStore.load() {
            Ok(Some(_)) => println!("Authenticated token present in OS credential store"),
            Ok(None) => println!("Not authenticated"),
            Err(error) => println!("Credential status unavailable: {error}"),
        },
        Command::Auth {
            action: AuthAction::Logout,
        } => {
            TokenStore.clear()?;
            println!("Stored TIDAL authorization removed");
        }
    }
    Ok(())
}

async fn run_doctor() -> Result<()> {
    let config = AppConfig::load()?;
    let client_id = config
        .client_id
        .context("public client ID is not configured")?;
    let mut tokens = TokenStore
        .load()?
        .context("no OAuth authorization is stored; run `tidalbar auth login`")?;
    if tokens.client_id != client_id {
        return Err(anyhow!(
            "stored authorization belongs to a different client ID"
        ));
    }
    if tokens.expires_soon() {
        tokens = refresh(&tokens).await?;
        TokenStore.save(&tokens)?;
    }
    let client = TidalClient::new(tokens.access_token);
    let mut failures = Vec::new();

    let search_items = match client.search("Miles Davis").await {
        Ok(items) => {
            println!("✓ search: {} items", items.len());
            items
        }
        Err(error) => {
            println!("✗ search: {error}");
            failures.push("search");
            Vec::new()
        }
    };

    macro_rules! check_items {
        ($label:literal, $request:expr) => {
            match $request.await {
                Ok(items) => println!("✓ {}: {} items", $label, items.len()),
                Err(error) => {
                    println!("✗ {}: {}", $label, error);
                    failures.push($label);
                }
            }
        };
    }

    check_items!("collection tracks", client.collection_tracks());
    check_items!("collection albums", client.collection_albums());
    check_items!("collection artists", client.collection_artists());
    check_items!("collection playlists", client.collection_playlists());
    check_items!("daily mixes", client.daily_mixes());
    check_items!("discovery mixes", client.discovery_mixes());
    check_items!("new release mixes", client.new_release_mixes());

    if let Some(track) = search_items
        .iter()
        .find(|item| item.kind == MediaKind::Track)
    {
        match client.official_preview(&track.id).await {
            Ok(_) => println!("✓ official preview manifest"),
            Err(tidalbar::tidal::TidalError::FullTrackDisabled) => {
                println!("✓ playback policy rejected a full-track manifest")
            }
            Err(error) => {
                println!("✗ official preview manifest: {error}");
                failures.push("official preview manifest");
            }
        }
    }

    if failures.is_empty() {
        println!("tidalbar API diagnostics passed");
        Ok(())
    } else {
        Err(anyhow!("failed checks: {}", failures.join(", ")))
    }
}

async fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    mut artwork: Option<&mut ArtworkState>,
    tidal: Option<&TidalClient>,
) -> Result<()> {
    let resolver = PreviewResolver;
    let mut player = MpvEngine::new();

    loop {
        terminal.draw(|frame| ui::draw(frame, app, artwork.as_deref_mut()))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };

        let selected_before = app.selected().map(|item| item.id.clone());
        match app.handle_key(key) {
            Action::None => {}
            Action::Quit => break,
            Action::Search(query) => {
                if let Some(client) = tidal {
                    match client.search(&query).await {
                        Ok(items) => app
                            .replace_shelves(vec![Shelf::new(format!("Search · {query}"), items)]),
                        Err(error) => app.playback_failed(error.to_string()),
                    }
                } else {
                    app.playback_failed("Authenticate with `tidalbar auth login` to search");
                }
            }
            Action::Load(screen) => {
                if let Some(client) = tidal {
                    match load_screen(client, screen).await {
                        Ok(shelves) => app.replace_shelves(shelves),
                        Err(error) => app.playback_failed(error.to_string()),
                    }
                } else {
                    app.playback_failed(
                        "Authenticate with `tidalbar auth login` to load TIDAL data",
                    );
                }
            }
            Action::Play(item) => {
                let resource = match resolver.resolve(&item) {
                    Ok(resource) => Ok(resource),
                    Err(error) if item.kind == MediaKind::Track => match tidal {
                        Some(client) => client
                            .official_preview(&item.id)
                            .await
                            .map_err(|error| error.to_string()),
                        None => Err(error.to_string()),
                    },
                    Err(error) => Err(error.to_string()),
                };
                match resource {
                    Ok(resource) => match player.play(&resource) {
                        Ok(()) => app.playback_started(item),
                        Err(error) => app.playback_failed(error.to_string()),
                    },
                    Err(error) => app.playback_failed(error),
                }
            }
            Action::TogglePause(paused) => {
                if let Err(error) = player.set_paused(paused) {
                    app.playback_failed(error.to_string());
                }
            }
        }

        let selected_after = app.selected().map(|item| item.id.clone());
        if selected_after != selected_before {
            update_artwork(tidal, app.selected(), artwork.as_deref_mut()).await;
        }
    }

    if let Err(error) = player.stop() {
        tracing::debug!(%error, "mpv was not running during shutdown");
    }
    Ok(())
}

async fn load_screen(client: &TidalClient, screen: Screen) -> Result<Vec<Shelf>> {
    match screen {
        Screen::ForYou => {
            let (daily, discovery, new_releases) = tokio::join!(
                client.daily_mixes(),
                client.discovery_mixes(),
                client.new_release_mixes()
            );
            let mixes = shelves_from_results([
                ("Daily mixes", daily),
                ("Discovery mixes", discovery),
                ("New releases", new_releases),
            ])?;
            if !mixes.is_empty() {
                return Ok(mixes);
            }

            let (tracks, albums, playlists) = tokio::join!(
                client.collection_tracks(),
                client.collection_albums(),
                client.collection_playlists()
            );
            shelves_from_results([
                ("Recently liked tracks", tracks),
                ("Recently liked albums", albums),
                ("Your playlists", playlists),
            ])
        }
        Screen::Collection => {
            let (tracks, albums, artists, playlists) = tokio::join!(
                client.collection_tracks(),
                client.collection_albums(),
                client.collection_artists(),
                client.collection_playlists()
            );
            shelves_from_results([
                ("Liked tracks", tracks),
                ("Liked albums", albums),
                ("Liked artists", artists),
                ("Collected playlists", playlists),
            ])
        }
        Screen::Playlists => Ok(vec![Shelf::new(
            "Your playlists",
            client.collection_playlists().await?,
        )]),
        Screen::Explore => Ok(vec![Shelf::new("Explore", Vec::new())]),
    }
}

fn shelves_from_results<const N: usize>(
    results: [(&str, Result<Vec<MediaItem>, tidalbar::tidal::TidalError>); N],
) -> Result<Vec<Shelf>> {
    let mut shelves = Vec::new();
    let mut errors = Vec::new();
    let mut successes = 0;
    for (title, result) in results {
        match result {
            Ok(items) => {
                successes += 1;
                if !items.is_empty() {
                    shelves.push(Shelf::new(title, items));
                }
            }
            Err(error) => errors.push(error.to_string()),
        }
    }
    if successes == 0 {
        Err(anyhow!(errors.join("; ")))
    } else {
        Ok(shelves)
    }
}

async fn update_artwork(
    client: Option<&TidalClient>,
    item: Option<&MediaItem>,
    artwork: Option<&mut ArtworkState>,
) {
    let (Some(client), Some(url), Some(artwork)) = (
        client,
        item.and_then(|item| item.artwork_url.as_deref()),
        artwork,
    ) else {
        return;
    };
    if let Ok(bytes) = client.artwork(url).await {
        let _ = artwork.load(&bytes);
    }
}
