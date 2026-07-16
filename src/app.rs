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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Sidebar,
    Content,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    FocusPlayer(bool),
    Load(Screen),
    None,
    Play(MediaItem),
    Quit,
    Search(String),
    TogglePause(bool),
}

#[derive(Debug)]
struct ViewState {
    screen: Screen,
    shelves: Vec<Shelf>,
    selected_shelf: usize,
    selected_item: usize,
    status: String,
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub shelves: Vec<Shelf>,
    pub selected_shelf: usize,
    pub selected_item: usize,
    pub search_active: bool,
    pub search_query: String,
    pub help_visible: bool,
    pub player_focused: bool,
    pub focus: Focus,
    pub now_playing: Option<MediaItem>,
    pub paused: bool,
    pub status: String,
    history: Vec<ViewState>,
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
            help_visible: false,
            player_focused: false,
            focus: Focus::Content,
            now_playing: None,
            paused: false,
            status: status.to_owned(),
            history: Vec::new(),
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

        if self.help_visible {
            if matches!(
                key.code,
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')
            ) {
                self.help_visible = false;
            }
            return Action::None;
        }

        if matches!(key.code, KeyCode::Char('?')) {
            self.help_visible = true;
            return Action::None;
        }
        if matches!(key.code, KeyCode::Char('f')) {
            self.player_focused = !self.player_focused;
            return Action::FocusPlayer(self.player_focused);
        }
        if self.player_focused && key.code == KeyCode::Esc {
            self.player_focused = false;
            return Action::FocusPlayer(false);
        }
        if self.player_focused && !matches!(key.code, KeyCode::Char('q') | KeyCode::Char(' ')) {
            return Action::None;
        }

        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.focus = match self.focus {
                Focus::Sidebar => Focus::Content,
                Focus::Content => Focus::Sidebar,
            };
            self.status = match self.focus {
                Focus::Sidebar => "Sidebar focused · ↑/↓ choose · Tab returns".to_owned(),
                Focus::Content => "Content focused · arrows or hjkl navigate".to_owned(),
            };
            return Action::None;
        }

        if self.focus == Focus::Sidebar {
            return match key.code {
                KeyCode::Char('/') => {
                    self.search_active = true;
                    self.status = "Search TIDAL".to_owned();
                    Action::None
                }
                KeyCode::Char('g') => self.switch_screen(Screen::ForYou),
                KeyCode::Char('e') => self.switch_screen(Screen::Explore),
                KeyCode::Char('c') => self.switch_screen(Screen::Collection),
                KeyCode::Char('P') => self.switch_screen(Screen::Playlists),
                KeyCode::Down | KeyCode::Char('j') => self.move_screen(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_screen(-1),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    self.focus = Focus::Content;
                    self.status = "Content focused · arrows or hjkl navigate".to_owned();
                    Action::None
                }
                KeyCode::Char('q') => Action::Quit,
                _ => Action::None,
            };
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
            KeyCode::Esc | KeyCode::Backspace => {
                self.go_back();
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_item(1);
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_item(-1);
                Action::None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_shelf(1);
                Action::None
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.move_shelf(-1);
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
        self.history.clear();
        self.shelves = shelves;
        self.selected_shelf = 0;
        self.selected_item = 0;
        self.status = "TIDAL data loaded".to_owned();
    }

    pub fn open_items(&mut self, title: impl Into<String>, items: Vec<MediaItem>) {
        self.history.push(ViewState {
            screen: self.screen,
            shelves: self.shelves.clone(),
            selected_shelf: self.selected_shelf,
            selected_item: self.selected_item,
            status: self.status.clone(),
        });
        self.shelves = vec![Shelf::new(title, items)];
        self.selected_shelf = 0;
        self.selected_item = 0;
        self.status = "Backspace returns to the previous view".to_owned();
    }

    pub fn playback_started(&mut self, item: MediaItem) {
        self.status = format!("Playing official preview · {}", item.title);
        self.now_playing = Some(item);
        self.paused = false;
    }

    pub fn playback_failed(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    fn go_back(&mut self) {
        let Some(previous) = self.history.pop() else {
            return;
        };
        self.screen = previous.screen;
        self.shelves = previous.shelves;
        self.selected_shelf = previous.selected_shelf;
        self.selected_item = previous.selected_item;
        self.status = previous.status;
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

    fn move_screen(&mut self, delta: isize) -> Action {
        let current = Screen::ALL
            .iter()
            .position(|screen| *screen == self.screen)
            .unwrap_or_default();
        self.switch_screen(Screen::ALL[shifted_index(current, delta, Screen::ALL.len())])
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

        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.selected_shelf, app.shelves.len() - 1);

        app.handle_key(key(KeyCode::Up));
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
    fn detail_views_restore_the_previous_selection() {
        let mut app = App::new(false);
        app.selected_item = 2;
        app.open_items(
            "Album",
            vec![MediaItem::new(
                "track",
                "Track",
                "Artist",
                crate::models::MediaKind::Track,
            )],
        );

        app.handle_key(key(KeyCode::Backspace));

        assert_eq!(app.selected_item, 2);
        assert_eq!(app.shelves[0].title, "Custom mixes");
    }

    #[test]
    fn help_overlay_captures_keys_until_closed() {
        let mut app = App::new(false);

        assert_eq!(app.handle_key(key(KeyCode::Char('?'))), Action::None);
        assert!(app.help_visible);
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::None);
        assert!(!app.help_visible);
    }

    #[test]
    fn tab_focuses_the_sidebar_and_arrows_load_views() {
        let mut app = App::new(false);

        app.handle_key(key(KeyCode::Tab));
        let action = app.handle_key(key(KeyCode::Down));

        assert_eq!(app.focus, Focus::Sidebar);
        assert_eq!(app.screen, Screen::Explore);
        assert_eq!(action, Action::Load(Screen::Explore));
    }

    #[test]
    fn player_focus_toggles_with_f_and_escape() {
        let mut app = App::new(false);

        assert_eq!(
            app.handle_key(key(KeyCode::Char('f'))),
            Action::FocusPlayer(true)
        );
        assert!(app.player_focused);
        assert_eq!(
            app.handle_key(key(KeyCode::Esc)),
            Action::FocusPlayer(false)
        );
        assert!(!app.player_focused);
    }

    #[test]
    fn selection_is_delegated_to_the_policy_aware_playback_layer() {
        let mut app = App::new(false);

        let action = app.handle_key(key(KeyCode::Enter));

        assert!(matches!(action, Action::Play(_)));
    }
}
