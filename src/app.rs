use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::models::{MediaItem, Shelf, demo_home};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    ForYou,
    Explore,
    Collection,
    Playlists,
}

impl Screen {
    pub const ALL: [Self; 4] = [
        Self::ForYou,
        Self::Explore,
        Self::Collection,
        Self::Playlists,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ForYou => "For You",
            Self::Explore => "Explore",
            Self::Collection => "Collection",
            Self::Playlists => "Playlists",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Load(Screen),
    None,
    Play(MediaItem),
    Quit,
    Search(String),
    TogglePause(bool),
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub shelves: Vec<Shelf>,
    pub selected_shelf: usize,
    pub selected_item: usize,
    pub search_active: bool,
    pub search_query: String,
    pub now_playing: Option<MediaItem>,
    pub paused: bool,
    pub status: String,
}

impl App {
    pub fn new(authenticated: bool) -> Self {
        let status = if authenticated {
            "Connected to TIDAL"
        } else {
            "Preview build · authenticate with `tidalbar auth login` when configured"
        };

        Self {
            screen: Screen::ForYou,
            shelves: demo_home(),
            selected_shelf: 0,
            selected_item: 0,
            search_active: false,
            search_query: String::new(),
            now_playing: None,
            paused: false,
            status: status.to_owned(),
        }
    }

    pub fn selected(&self) -> Option<&MediaItem> {
        self.shelves
            .get(self.selected_shelf)
            .and_then(|shelf| shelf.items.get(self.selected_item))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.kind != KeyEventKind::Press {
            return Action::None;
        }

        if self.search_active {
            return self.handle_search_key(key.code);
        }

        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('/') => {
                self.search_active = true;
                self.status = "Search TIDAL".to_owned();
                Action::None
            }
            KeyCode::Char('g') => self.switch_screen(Screen::ForYou),
            KeyCode::Char('e') => self.switch_screen(Screen::Explore),
            KeyCode::Char('c') => self.switch_screen(Screen::Collection),
            KeyCode::Char('P') => self.switch_screen(Screen::Playlists),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_shelf(1);
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_shelf(-1);
                Action::None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_item(1);
                Action::None
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.move_item(-1);
                Action::None
            }
            KeyCode::Enter | KeyCode::Char('p') => self.play_selected(),
            KeyCode::Char(' ') => {
                if self.now_playing.is_some() {
                    self.paused = !self.paused;
                    Action::TogglePause(self.paused)
                } else {
                    self.status = "Nothing is playing".to_owned();
                    Action::None
                }
            }
            _ => Action::None,
        }
    }

    pub fn replace_shelves(&mut self, shelves: Vec<Shelf>) {
        self.shelves = shelves;
        self.selected_shelf = 0;
        self.selected_item = 0;
        self.status = "TIDAL data loaded".to_owned();
    }

    pub fn playback_started(&mut self, item: MediaItem) {
        self.status = format!("Playing official preview · {}", item.title);
        self.now_playing = Some(item);
        self.paused = false;
    }

    pub fn playback_failed(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    fn handle_search_key(&mut self, code: KeyCode) -> Action {
        match code {
            KeyCode::Esc => {
                self.search_active = false;
                self.status = "Search cancelled".to_owned();
            }
            KeyCode::Enter => {
                self.search_active = false;
                if self.search_query.is_empty() {
                    self.status = "Enter a search query".to_owned();
                } else {
                    self.status = format!("Searching · {}", self.search_query);
                    return Action::Search(self.search_query.clone());
                }
            }
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Char(character) => self.search_query.push(character),
            _ => {}
        }
        Action::None
    }

    fn switch_screen(&mut self, screen: Screen) -> Action {
        self.screen = screen;
        self.selected_shelf = 0;
        self.selected_item = 0;
        self.status = format!("Loading {}", screen.label());
        Action::Load(screen)
    }

    fn move_shelf(&mut self, delta: isize) {
        if self.shelves.is_empty() {
            return;
        }
        self.selected_shelf = shifted_index(self.selected_shelf, delta, self.shelves.len());
        self.selected_item = 0;
    }

    fn move_item(&mut self, delta: isize) {
        let Some(shelf) = self.shelves.get(self.selected_shelf) else {
            return;
        };
        if shelf.items.is_empty() {
            return;
        }
        self.selected_item = shifted_index(self.selected_item, delta, shelf.items.len());
    }

    fn play_selected(&mut self) -> Action {
        let Some(item) = self.selected().cloned() else {
            return Action::None;
        };
        Action::Play(item)
    }
}

fn shifted_index(current: usize, delta: isize, len: usize) -> usize {
    (current as isize + delta).rem_euclid(len as isize) as usize
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn navigation_wraps_between_shelves_and_items() {
        let mut app = App::new(false);

        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.selected_shelf, app.shelves.len() - 1);

        app.handle_key(key(KeyCode::Left));
        assert_eq!(
            app.selected_item,
            app.shelves.last().expect("demo shelf").items.len() - 1
        );
    }

    #[test]
    fn search_captures_text_without_triggering_shortcuts() {
        let mut app = App::new(false);

        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Char('q')));
        let action = app.handle_key(key(KeyCode::Enter));

        assert!(!app.search_active);
        assert_eq!(app.search_query, "q");
        assert_eq!(action, Action::Search("q".to_owned()));
    }

    #[test]
    fn selection_is_delegated_to_the_policy_aware_playback_layer() {
        let mut app = App::new(false);

        let action = app.handle_key(key(KeyCode::Enter));

        assert!(matches!(action, Action::Play(_)));
    }
}
