use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::models::{MediaItem, MediaKind, PlaybackProgress, Shelf, demo_home};
use crate::queue::PlaybackQueue;
use crate::song_list::SongListOrder;

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
    SetLiked { track: MediaItem, liked: bool },
    TogglePause(bool),
}

#[derive(Debug)]
struct ViewState {
    screen: Screen,
    shelves: Vec<Shelf>,
    selected_shelf: usize,
    selected_item: usize,
    status: String,
    list_orders: Vec<SongListOrder>,
    selection_explicit: bool,
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
    pub shuffle: bool,
    pub queue: PlaybackQueue,
    pub status: String,
    pub progress: PlaybackProgress,
    /// `None` until the collection has been loaded.
    liked_tracks: Option<HashSet<String>>,
    /// Likes changed while the collection was still loading.
    liked_changes: HashMap<String, bool>,
    history: Vec<ViewState>,
    list_orders: Vec<SongListOrder>,
    selection_explicit: bool,
}

impl App {
    pub fn new(authenticated: bool) -> Self {
        let status = if authenticated {
            "Connected to TIDAL"
        } else {
            "Not connected · configure TIDAL, then run `tidalbar auth login`"
        };

        let shelves = demo_home();
        let list_orders = shelves.iter().map(SongListOrder::new).collect();
        Self {
            screen: Screen::ForYou,
            shelves,
            selected_shelf: 0,
            selected_item: 0,
            search_active: false,
            search_query: String::new(),
            help_visible: false,
            player_focused: false,
            focus: Focus::Content,
            now_playing: None,
            paused: false,
            shuffle: false,
            queue: PlaybackQueue::default(),
            status: status.to_owned(),
            progress: PlaybackProgress::default(),
            liked_tracks: None,
            liked_changes: HashMap::new(),
            history: Vec::new(),
            list_orders,
            selection_explicit: false,
        }
    }

    pub fn selected(&self) -> Option<&MediaItem> {
        self.shelves
            .get(self.selected_shelf)
            .and_then(|shelf| shelf.items.get(self.selected_item))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.handle_key_with_rng(key, &mut rand::rng())
    }

    fn handle_key_with_rng(&mut self, key: KeyEvent, rng: &mut impl rand::Rng) -> Action {
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
        if key.code == KeyCode::Char('s') {
            self.toggle_shuffle(rng);
            return Action::None;
        }
        if key.code == KeyCode::Char('n') {
            return self.next_track();
        }
        if key.code == KeyCode::Char('L') {
            return self.toggle_like();
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
            KeyCode::Char('S') => self.shuffle_list(rng),
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
        self.install_shelves(shelves);
        self.status = "TIDAL data loaded".to_owned();
    }

    pub fn open_items(&mut self, title: impl Into<String>, items: Vec<MediaItem>) {
        self.history.push(ViewState {
            screen: self.screen,
            shelves: self.shelves.clone(),
            selected_shelf: self.selected_shelf,
            selected_item: self.selected_item,
            status: self.status.clone(),
            list_orders: self.list_orders.clone(),
            selection_explicit: self.selection_explicit,
        });
        self.install_shelves(vec![Shelf::new(title, items)]);
        self.status = "Backspace returns to the previous view".to_owned();
    }

    fn install_shelves(&mut self, shelves: Vec<Shelf>) {
        self.install_shelves_with_rng(shelves, &mut rand::rng());
    }

    fn install_shelves_with_rng(&mut self, shelves: Vec<Shelf>, rng: &mut impl rand::Rng) {
        self.list_orders = shelves.iter().map(SongListOrder::new).collect();
        self.shelves = shelves;
        self.selected_shelf = 0;
        self.selected_item = 0;
        self.selection_explicit = false;
        self.apply_view_shuffle(rng);
        self.align_view_with_queue();
    }

    fn default_item(&self) -> usize {
        if self.shuffle {
            self.shelves
                .get(self.selected_shelf)
                .and_then(|shelf| {
                    shelf
                        .items
                        .iter()
                        .position(|item| item.kind == MediaKind::Track)
                })
                .unwrap_or(0)
        } else {
            0
        }
    }

    fn apply_view_shuffle(&mut self, rng: &mut impl rand::Rng) {
        let selected_original = self
            .list_orders
            .get(self.selected_shelf)
            .and_then(|order| order.original_position(self.selected_item));
        for (shelf, order) in self.shelves.iter_mut().zip(&mut self.list_orders) {
            order.set_shuffle(shelf, self.shuffle, rng);
        }
        self.selected_item = if self.shuffle && !self.selection_explicit {
            self.default_item()
        } else {
            selected_original
                .and_then(|original| {
                    self.list_orders
                        .get(self.selected_shelf)
                        .and_then(|order| order.displayed_position(original))
                })
                .unwrap_or(0)
        };
    }

    fn align_view_with_queue(&mut self) {
        if !self.shuffle {
            return;
        }
        for (index, (shelf, order)) in self
            .shelves
            .iter_mut()
            .zip(&mut self.list_orders)
            .enumerate()
        {
            let (tracks, _) = order.queue_plan(shelf);
            if !self.queue.matches_tracks(&tracks) {
                continue;
            }
            let selected_original = order.original_position(self.selected_item);
            if let Some((queued_order, position)) = self.queue.visible_order() {
                order.set_track_order(shelf, queued_order);
                if index == self.selected_shelf {
                    self.selected_item = if self.selection_explicit {
                        selected_original
                            .and_then(|original| order.displayed_position(original))
                            .unwrap_or(0)
                    } else {
                        shelf
                            .items
                            .iter()
                            .enumerate()
                            .filter(|(_, item)| item.kind == MediaKind::Track)
                            .nth(position)
                            .map(|(index, _)| index)
                            .unwrap_or(0)
                    };
                }
            }
        }
    }

    fn toggle_shuffle(&mut self, rng: &mut impl rand::Rng) {
        self.shuffle = !self.shuffle;
        self.apply_view_shuffle(rng);
        // When this view contains the queue's source list, use precisely the
        // visible permutation rather than independently shuffling the queue.
        let displayed_order =
            self.shelves
                .iter()
                .zip(&self.list_orders)
                .find_map(|(shelf, order)| {
                    let (tracks, displayed) = order.queue_plan(shelf);
                    self.queue.matches_tracks(&tracks).then_some(displayed)
                });
        if let Some(order) = displayed_order {
            self.queue.set_display_order(&order);
        } else {
            self.queue.set_shuffle(self.shuffle, rng);
        }
        self.status = if !self.shuffle {
            "Shuffle off · original song order restored"
        } else if self.selected_tracks().is_empty() {
            "Shuffle on · open a song list"
        } else {
            "Shuffle on · Enter plays highlighted song"
        }
        .to_owned();
    }

    /// The song shown in player focus: what is playing, else the highlight.
    pub fn focused_item(&self) -> Option<&MediaItem> {
        self.now_playing.as_ref().or_else(|| self.selected())
    }

    /// `None` while the liked-track collection is unknown.
    pub fn is_liked(&self, track_id: &str) -> Option<bool> {
        if let Some(liked) = self.liked_changes.get(track_id) {
            return Some(*liked);
        }
        self.liked_tracks
            .as_ref()
            .map(|tracks| tracks.contains(track_id))
    }

    pub fn liked_tracks_loaded(&mut self, mut tracks: HashSet<String>) {
        for (id, liked) in self.liked_changes.drain() {
            if liked {
                tracks.insert(id);
            } else {
                tracks.remove(&id);
            }
        }
        self.liked_tracks = Some(tracks);
    }

    pub fn like_saved(&mut self, track: &MediaItem, liked: bool) {
        match self.liked_tracks.as_mut() {
            Some(tracks) if liked => {
                tracks.insert(track.id.clone());
            }
            Some(tracks) => {
                tracks.remove(&track.id);
            }
            None => {
                self.liked_changes.insert(track.id.clone(), liked);
            }
        }
        self.status = if liked {
            format!("♥ Liked · {}", track.title)
        } else {
            format!("Removed from Liked · {}", track.title)
        };
    }

    fn toggle_like(&mut self) -> Action {
        let Some(track) = self
            .focused_item()
            .filter(|item| item.kind == MediaKind::Track)
            .cloned()
        else {
            self.status = "Play or highlight a song to like it".to_owned();
            return Action::None;
        };
        // An unknown state likes: adding an already-liked song is harmless.
        let liked = self.is_liked(&track.id) != Some(true);
        Action::SetLiked { track, liked }
    }

    pub fn playback_started(&mut self, item: MediaItem) {
        self.status = format!("Playing · {}", item.title);
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
        self.list_orders = previous.list_orders;
        self.selection_explicit = previous.selection_explicit;
        self.apply_view_shuffle(&mut rand::rng());
        self.align_view_with_queue();
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
        self.selected_item = self.default_item();
        self.selection_explicit = false;
    }

    fn move_item(&mut self, delta: isize) {
        let Some(shelf) = self.shelves.get(self.selected_shelf) else {
            return;
        };
        if shelf.items.is_empty() {
            return;
        }
        self.selected_item = shifted_index(self.selected_item, delta, shelf.items.len());
        self.selection_explicit = true;
    }

    /// Natural EOF advances the captured list, even when a different view is open.
    pub fn playback_finished(&mut self) -> Action {
        self.now_playing = None;
        self.paused = false;
        self.next_track()
    }

    pub fn next_track(&mut self) -> Action {
        match self.queue.advance() {
            Some(track) => Action::Play(track),
            None => {
                self.status = "End of song list".to_owned();
                Action::None
            }
        }
    }

    fn selected_tracks(&self) -> Vec<MediaItem> {
        self.shelves
            .get(self.selected_shelf)
            .into_iter()
            .flat_map(|shelf| &shelf.items)
            .filter(|item| item.kind == MediaKind::Track)
            .cloned()
            .collect()
    }

    fn shuffle_list(&mut self, rng: &mut impl rand::Rng) -> Action {
        let tracks = self.selected_tracks();
        if tracks.is_empty() {
            self.status = "No songs in this list · open an album or playlist first".to_owned();
            return Action::None;
        }
        self.shuffle = true;
        self.selection_explicit = false;
        // S is a fresh shuffle even when the mode is already enabled.
        // Shuffle this shelf once, then bring the other shelves into the mode.
        self.list_orders[self.selected_shelf]
            .reshuffle(&mut self.shelves[self.selected_shelf], rng);
        self.apply_view_shuffle(rng);
        self.selected_item = self.default_item();
        self.play_selected()
    }

    fn play_selected(&mut self) -> Action {
        let Some(item) = self.selected().cloned() else {
            return Action::None;
        };
        if item.kind == MediaKind::Track {
            let shelf = &self.shelves[self.selected_shelf];
            let (tracks, order) = self.list_orders[self.selected_shelf].queue_plan(shelf);
            // Use occurrence positions, not IDs: playlists can contain duplicates.
            let selected = shelf.items[..self.selected_item]
                .iter()
                .filter(|item| item.kind == MediaKind::Track)
                .count();
            self.queue
                .start_ordered(tracks, order, selected, self.shuffle);
        }
        Action::Play(item)
    }
}

fn shifted_index(current: usize, delta: isize, len: usize) -> usize {
    (current as isize + delta).rem_euclid(len as isize) as usize
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use rand::{SeedableRng, rngs::StdRng};

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
    fn playback_status_describes_the_track_without_claiming_it_is_a_preview() {
        let mut app = App::new(true);
        let track = MediaItem::new("track", "Track", "Artist", crate::models::MediaKind::Track);

        app.playback_started(track.clone());

        assert_eq!(app.now_playing, Some(track));
        assert_eq!(app.status, "Playing · Track");
    }

    fn songs() -> Vec<MediaItem> {
        (0..6)
            .map(|i| {
                MediaItem::new(
                    i.to_string(),
                    format!("Song {i}"),
                    "Artist",
                    MediaKind::Track,
                )
            })
            .collect()
    }

    fn played_id(action: Action) -> String {
        match action {
            Action::Play(track) => {
                assert_eq!(track.kind, MediaKind::Track);
                track.id
            }
            other => panic!("expected a song, got {other:?}"),
        }
    }

    #[test]
    fn shuffle_works_for_every_song_list_source_and_survives_browsing() {
        for title in [
            "Playlist",
            "Search",
            "Album",
            "Collection",
            "Artist",
            "Radio",
        ] {
            let mut app = App::new(true);
            app.open_items(title, songs());
            let original = app.shelves.clone();
            app.handle_key_with_rng(key(KeyCode::Char('s')), &mut StdRng::seed_from_u64(42));
            assert_ne!(app.shelves, original, "shuffle must be visible");
            app.selected_item = 3;
            let first = app.handle_key(key(KeyCode::Enter));
            let first_id = played_id(first.clone());
            assert_eq!(first_id, app.selected().unwrap().id);
            if let Action::Play(track) = first {
                app.playback_started(track);
            }
            app.handle_key(key(KeyCode::Backspace));
            app.replace_shelves(vec![Shelf::new("Other list", vec![])]);
            let mut ids = vec![first_id];
            for _ in 1..6 {
                ids.push(played_id(app.playback_finished()));
            }
            ids.sort();
            assert_eq!(ids, ["0", "1", "2", "3", "4", "5"]);
            assert_eq!(app.playback_finished(), Action::None);
            assert_eq!(app.now_playing, None);
            assert!(!app.paused);
            assert!(app.shuffle);
            assert_eq!(app.status, "End of song list");
        }
    }

    #[test]
    fn s_then_play_shuffles_the_visible_playlist_and_starts_at_its_random_first_song() {
        let mut starting_ids = std::collections::HashSet::new();
        for seed in 0..32 {
            let mut app = App::new(true);
            app.open_items("Playlist", songs());
            app.handle_key_with_rng(key(KeyCode::Char('s')), &mut StdRng::seed_from_u64(seed));
            let displayed: Vec<_> = app.shelves[0]
                .items
                .iter()
                .map(|item| item.id.clone())
                .collect();
            assert_eq!(app.selected_item, 0);
            let first_id = played_id(app.handle_key(key(KeyCode::Char('p'))));
            assert_eq!(first_id, displayed[0]);
            starting_ids.insert(first_id.clone());
            let mut played = vec![first_id];
            while let Action::Play(track) = app.playback_finished() {
                played.push(track.id);
            }
            assert_eq!(
                played, displayed,
                "playback must follow the displayed shuffle, not reshuffle it"
            );
            app.handle_key(key(KeyCode::Char('s')));
            assert_eq!(
                app.shelves[0].items,
                songs(),
                "shuffle-off restores provider order"
            );
        }
        assert_eq!(
            starting_ids.len(),
            6,
            "every song, not just the first, can start playback"
        );
    }

    #[test]
    fn an_explicit_selection_survives_shuffle_but_the_remaining_queue_matches_the_list() {
        let mut app = App::new(true);
        app.open_items("Playlist", songs());
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected().unwrap().id, "2");
        app.handle_key_with_rng(key(KeyCode::Char('s')), &mut StdRng::seed_from_u64(42));
        assert_eq!(app.selected().unwrap().id, "2");
        let displayed: Vec<_> = app.shelves[0]
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect();
        let selected = app.selected_item;
        let expected: Vec<_> = displayed[selected..]
            .iter()
            .chain(&displayed[..selected])
            .cloned()
            .collect();
        let mut played = vec![played_id(app.handle_key(key(KeyCode::Enter)))];
        while let Action::Play(track) = app.playback_finished() {
            played.push(track.id);
        }
        assert_eq!(played, expected);
    }

    #[test]
    fn mid_playback_toggle_keeps_current_song_and_aligns_unplayed_songs_with_display() {
        let mut app = App::new(true);
        app.open_items("Playlist", songs());
        let Action::Play(first) = app.handle_key(key(KeyCode::Enter)) else {
            panic!("play")
        };
        app.playback_started(first);
        let Action::Play(second) = app.next_track() else {
            panic!("next")
        };
        app.playback_started(second.clone());
        app.handle_key_with_rng(key(KeyCode::Char('s')), &mut StdRng::seed_from_u64(42));
        assert_eq!(app.now_playing, Some(second));
        let expected: Vec<_> = app.shelves[0]
            .items
            .iter()
            .filter(|item| item.id != "0" && item.id != "1")
            .map(|item| item.id.clone())
            .collect();
        let next = played_id(app.next_track());
        assert_eq!(next, expected[0]);
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.shelves[0].items, songs());
        let expected: Vec<_> = songs()
            .into_iter()
            .filter(|item| item.id != "0" && item.id != "1" && item.id != next)
            .map(|item| item.id)
            .collect();
        let mut played = vec![];
        while let Action::Play(track) = app.playback_finished() {
            played.push(track.id);
        }
        assert_eq!(
            played, expected,
            "shuffle-off must not repeat previously played songs"
        );
    }

    #[test]
    fn mode_applies_to_new_lists_and_back_navigation_restores_original_order() {
        let mut app = App::new(true);
        app.open_items("Parent", songs());
        let parent = app.shelves.clone();
        app.open_items("Child", songs());
        app.handle_key_with_rng(key(KeyCode::Char('s')), &mut StdRng::seed_from_u64(42));
        let shuffled_child = app.shelves.clone();
        app.open_items("Grandchild", songs());
        assert!(app.shuffle);
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(
            app.shelves, shuffled_child,
            "returning within the same mode must not reshuffle"
        );
        app.handle_key(key(KeyCode::Char('s')));
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.shelves, parent);
        app.handle_key(key(KeyCode::Char('s')));
        app.install_shelves_with_rng(
            vec![Shelf::new("Search", songs())],
            &mut StdRng::seed_from_u64(42),
        );
        assert_ne!(app.shelves[0].items, songs());
        let (canonical, order) = app.list_orders[0].queue_plan(&app.shelves[0]);
        assert_eq!(canonical, songs());
        assert_eq!(app.selected_item, 0);
        let displayed: Vec<_> = app.shelves[0]
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect();
        assert_eq!(played_id(app.handle_key(key(KeyCode::Enter))), displayed[0]);
        assert_eq!(order.len(), 6);
    }

    #[test]
    fn returning_to_the_active_list_displays_its_existing_queue_without_reshuffling_playback() {
        let mut app = App::new(true);
        app.open_items("Parent", songs());
        let Action::Play(first) = app.handle_key(key(KeyCode::Enter)) else {
            panic!("play")
        };
        app.playback_started(first);
        let Action::Play(second) = app.next_track() else {
            panic!("next")
        };
        app.playback_started(second.clone());
        app.open_items(
            "Other",
            vec![MediaItem::new("other", "Other", "Artist", MediaKind::Track)],
        );
        app.handle_key_with_rng(key(KeyCode::Char('s')), &mut StdRng::seed_from_u64(42));
        let queued_order = app.queue.visible_order().unwrap().0.to_vec();
        app.handle_key(key(KeyCode::Backspace));
        let (_, displayed_order) = app.list_orders[0].queue_plan(&app.shelves[0]);
        assert_eq!(displayed_order, queued_order);
        assert_eq!(app.queue.visible_order().unwrap().0, queued_order);
        assert_eq!(app.now_playing, Some(second));
        let expected: Vec<_> = app.shelves[0]
            .items
            .iter()
            .filter(|item| item.id != "0" && item.id != "1")
            .map(|item| item.id.clone())
            .collect();
        let mut played = vec![];
        while let Action::Play(track) = app.playback_finished() {
            played.push(track.id);
        }
        assert_eq!(played, expected);
    }

    #[test]
    fn mixed_search_results_queue_only_tracks_in_the_selected_shelf() {
        let mut app = App::new(true);
        let mut items = songs();
        items.insert(
            1,
            MediaItem::new("album", "Album", "Artist", MediaKind::Album),
        );
        app.replace_shelves(vec![
            Shelf::new("Search", items),
            Shelf::new("Other", songs()),
        ]);
        app.selected_item = 3;
        assert_eq!(played_id(app.handle_key(key(KeyCode::Enter))), "2");
        assert_eq!(app.queue.progress(), Some((1, 4)));
        for expected in ["3", "4", "5"] {
            assert_eq!(played_id(app.playback_finished()), expected);
        }
        assert_eq!(app.playback_finished(), Action::None);
        app.selected_item = 1;
        assert!(matches!(
            app.handle_key(key(KeyCode::Enter)),
            Action::Play(MediaItem {
                kind: MediaKind::Album,
                ..
            })
        ));
        assert_eq!(
            app.queue.progress(),
            None,
            "opening an album must not create a queue"
        );
    }

    #[test]
    fn shuffle_list_handles_empty_and_non_track_lists_without_disturbing_playback() {
        let mut app = App::new(true);
        app.replace_shelves(vec![Shelf::new("Songs", songs())]);
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('S'))),
            Action::Play(_)
        ));
        assert!(app.shuffle);
        assert_eq!(app.queue.progress(), Some((1, 6)));
        app.replace_shelves(vec![]);
        assert_eq!(app.handle_key(key(KeyCode::Char('S'))), Action::None);
        assert_eq!(app.queue.progress(), Some((1, 6)));
        app.open_items(
            "Album list",
            vec![MediaItem::new("a", "Album", "Artist", MediaKind::Album)],
        );
        assert_eq!(app.handle_key(key(KeyCode::Char('S'))), Action::None);
        assert_eq!(app.queue.progress(), Some((1, 6)));
        assert!(app.status.contains("No songs"));
    }

    #[test]
    fn shuffle_and_next_are_available_in_sidebar_and_player_focus() {
        let mut app = App::new(true);
        app.replace_shelves(vec![Shelf::new("Songs", songs())]);
        let Action::Play(track) = app.handle_key(key(KeyCode::Enter)) else {
            panic!("song")
        };
        app.playback_started(track);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.handle_key(key(KeyCode::Char('s'))), Action::None);
        assert!(app.shuffle);
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('n'))),
            Action::Play(_)
        ));
        app.handle_key(key(KeyCode::Char('f')));
        app.handle_key(key(KeyCode::Char('s')));
        assert!(!app.shuffle);
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('n'))),
            Action::Play(_)
        ));
    }

    #[test]
    fn search_and_help_capture_shuffle_shortcuts() {
        let mut app = App::new(true);
        app.handle_key(key(KeyCode::Char('/')));
        for character in ['s', 'S', 'n'] {
            app.handle_key(key(KeyCode::Char(character)));
        }
        assert_eq!(app.search_query, "sSn");
        assert!(!app.shuffle);
        app.handle_key(key(KeyCode::Esc));
        app.handle_key(key(KeyCode::Char('?')));
        assert_eq!(app.handle_key(key(KeyCode::Char('s'))), Action::None);
        assert!(!app.shuffle);
        assert_eq!(app.queue.progress(), None);
    }

    #[test]
    fn duplicate_tracks_use_the_selected_occurrence() {
        let mut app = App::new(true);
        let mut items = songs();
        items[4] = items[0].clone();
        app.open_items("Duplicates", items);
        app.selected_item = 4;
        assert_eq!(played_id(app.handle_key(key(KeyCode::Enter))), "0");
        assert_eq!(app.queue.progress(), Some((1, 2)));
        assert_eq!(played_id(app.playback_finished()), "5");
        assert_eq!(app.playback_finished(), Action::None);
    }

    #[test]
    fn toggling_shuffle_preserves_the_explicit_duplicate_occurrence() {
        let mut app = App::new(true);
        let mut items = songs();
        items[4] = items[0].clone();
        app.open_items("Duplicates", items.clone());
        for _ in 0..4 {
            app.handle_key(key(KeyCode::Down));
        }
        app.handle_key_with_rng(key(KeyCode::Char('s')), &mut StdRng::seed_from_u64(42));
        assert_eq!(
            app.list_orders[0].original_position(app.selected_item),
            Some(4)
        );
        assert_eq!(played_id(app.handle_key(key(KeyCode::Enter))), "0");
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.selected_item, 4);
        assert_eq!(app.shelves[0].items, items);
        let mut remaining = vec![];
        while let Action::Play(track) = app.playback_finished() {
            remaining.push(track.id);
        }
        assert_eq!(
            remaining,
            ["0", "1", "2", "3", "5"],
            "the earlier duplicate is a distinct unplayed occurrence"
        );
    }

    #[test]
    fn starting_another_song_replaces_the_active_queue() {
        let mut app = App::new(true);
        app.open_items("First", songs());
        app.handle_key(key(KeyCode::Char('S')));
        let only_song = MediaItem::new("new", "New song", "Artist", MediaKind::Track);
        app.open_items("Second", vec![only_song.clone()]);
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::Play(only_song));
        assert_eq!(app.queue.progress(), Some((1, 1)));
        assert_eq!(app.playback_finished(), Action::None);
    }

    fn like_action(app: &mut App) -> Action {
        app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT))
    }

    #[test]
    fn like_toggles_the_playing_song_before_the_highlighted_one() {
        let mut app = App::new(true);
        app.open_items("Songs", songs());
        let playing = songs()[3].clone();
        app.playback_started(playing.clone());
        app.liked_tracks_loaded(HashSet::from(["3".to_owned()]));
        assert_eq!(
            like_action(&mut app),
            Action::SetLiked {
                track: playing.clone(),
                liked: false
            }
        );
        app.like_saved(&playing, false);
        assert_eq!(app.is_liked("3"), Some(false));
        assert_eq!(app.status, "Removed from Liked · Song 3");
        assert_eq!(
            like_action(&mut app),
            Action::SetLiked {
                track: playing.clone(),
                liked: true
            }
        );
        app.like_saved(&playing, true);
        assert_eq!(app.is_liked("3"), Some(true));
        assert_eq!(app.status, "♥ Liked · Song 3");
    }

    #[test]
    fn like_targets_the_highlighted_song_when_idle_and_rejects_non_songs() {
        let mut app = App::new(true);
        app.open_items("Songs", songs());
        app.selected_item = 2;
        assert_eq!(
            like_action(&mut app),
            Action::SetLiked {
                track: songs()[2].clone(),
                liked: true
            },
            "an unknown collection likes rather than unlikes"
        );
        app.open_items(
            "Albums",
            vec![MediaItem::new("a", "Album", "Artist", MediaKind::Album)],
        );
        assert_eq!(like_action(&mut app), Action::None);
        assert!(app.status.contains("highlight a song"));
    }

    #[test]
    fn like_works_from_sidebar_and_player_focus_but_not_search_or_help() {
        let mut app = App::new(true);
        app.open_items("Songs", songs());
        app.handle_key(key(KeyCode::Tab));
        assert!(matches!(like_action(&mut app), Action::SetLiked { .. }));
        app.handle_key(key(KeyCode::Char('f')));
        assert!(matches!(like_action(&mut app), Action::SetLiked { .. }));
        app.handle_key(key(KeyCode::Char('?')));
        assert_eq!(like_action(&mut app), Action::None);
        app.handle_key(key(KeyCode::Esc));
        app.handle_key(key(KeyCode::Char('f')));
        app.handle_key(key(KeyCode::Char('/')));
        assert_eq!(like_action(&mut app), Action::None);
        assert_eq!(app.search_query, "L");
    }

    #[test]
    fn likes_saved_before_the_collection_loads_override_the_stale_snapshot() {
        let mut app = App::new(true);
        let liked = songs()[0].clone();
        let unliked = songs()[1].clone();
        assert_eq!(app.is_liked("0"), None);
        app.like_saved(&liked, true);
        app.like_saved(&unliked, false);
        assert_eq!(app.is_liked("0"), Some(true));
        app.liked_tracks_loaded(HashSet::from(["1".to_owned(), "2".to_owned()]));
        assert_eq!(app.is_liked("0"), Some(true));
        assert_eq!(app.is_liked("1"), Some(false));
        assert_eq!(app.is_liked("2"), Some(true));
        assert_eq!(app.is_liked("5"), Some(false));
    }

    #[test]
    fn selection_is_delegated_to_the_policy_aware_playback_layer() {
        let mut app = App::new(false);

        let action = app.handle_key(key(KeyCode::Enter));

        assert!(matches!(action, Action::Play(_)));
    }
}
