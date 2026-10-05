use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, symbols};

use crate::app::{App, Focus, Screen};
use crate::artwork::ArtworkState;

const ACCENT: Color = Color::Rgb(0, 230, 205);
const PANEL: Color = Color::Rgb(24, 24, 24);
const MUTED: Color = Color::Rgb(145, 145, 145);

pub fn draw(frame: &mut Frame<'_>, app: &App, mut artwork: Option<&mut ArtworkState>) {
    let area = frame.area();
    if app.player_focused {
        render_player_focus(frame, area, app, artwork.as_deref_mut());
        if app.help_visible {
            render_help(frame, area);
        }
        return;
    }

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(4)])
        .split(area);

    let show_artwork = area.width >= 118 && artwork.is_some();
    let body_constraints = if show_artwork {
        vec![
            Constraint::Length(22),
            Constraint::Min(45),
            Constraint::Length(28),
        ]
    } else {
        vec![Constraint::Length(20), Constraint::Min(40)]
    };
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(body_constraints)
        .split(vertical[0]);

    render_sidebar(frame, body[0], app);
    render_content(frame, body[1], app);
    if let Some(artwork) = artwork.filter(|_| show_artwork) {
        render_artwork(frame, body[2], artwork);
    }
    render_player(frame, vertical[1], app);

    if app.search_active {
        render_search(frame, area, app);
    }
    if app.help_visible {
        render_help(frame, area);
    }
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let items = Screen::ALL.into_iter().map(|screen| {
        let marker = if screen == app.screen { "◆ " } else { "  " };
        let style = if screen == app.screen {
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        ListItem::new(format!("{marker}{}", screen.label())).style(style)
    });
    let border = if app.focus == Focus::Sidebar {
        ACCENT
    } else {
        PANEL
    };
    let list = List::new(items).block(
        Block::default()
            .title(" tidalbar · Tab ")
            .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_set(symbols::border::PLAIN)
            .border_style(Style::default().fg(border))
            .style(Style::default().bg(Color::Black)),
    );
    frame.render_widget(list, area);
}

fn render_content(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    let heading = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {}", app.screen.label()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "   / Search  ·  ? Help  ·  Tab Sidebar",
            Style::default().fg(MUTED),
        ),
    ]))
    .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(heading, content[0]);

    render_shelves(frame, content[1], app);

    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::default().fg(MUTED)),
        content[2],
    );
}

fn render_shelves(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.shelves.is_empty() {
        let message = match app.screen {
            Screen::Explore => {
                "Play a track, then reopen Explore to see its radio and similar tracks."
            }
            _ => "No recommendations are available.",
        };
        render_empty(frame, area, app.screen.label(), message);
        return;
    }

    let constraints = app
        .shelves
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if index == app.selected_shelf {
                Constraint::Min(3)
            } else {
                Constraint::Length(1)
            }
        })
        .collect::<Vec<_>>();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (row, (shelf_index, shelf)) in rows.iter().zip(app.shelves.iter().enumerate()) {
        let selected_shelf = shelf_index == app.selected_shelf;
        let title_style = if selected_shelf {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };

        if !selected_shelf {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!(" {} ({})", shelf.title, shelf.items.len()),
                    title_style,
                )),
                *row,
            );
            continue;
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(*row);
        let shuffled = app.shuffle
            && shelf
                .items
                .iter()
                .any(|item| item.kind == crate::models::MediaKind::Track);
        let title = if shuffled {
            format!(" {} · shuffled song order", shelf.title)
        } else {
            format!(" {}", shelf.title)
        };
        frame.render_widget(
            Paragraph::new(Line::styled(title, title_style)),
            sections[0],
        );

        let items: Vec<ListItem> = shelf
            .items
            .iter()
            .map(|item| ListItem::new(format!(" {} · {}", item.title, item.subtitle)))
            .collect();
        let list = List::new(items).highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        );
        let mut state = ListState::default();
        state.select(Some(app.selected_item));
        frame.render_stateful_widget(list, sections[1], &mut state);
    }
}

fn render_empty(frame: &mut Frame<'_>, area: Rect, title: &str, message: &str) {
    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL));
    let paragraph = Paragraph::new(message)
        .style(Style::default().fg(MUTED))
        .alignment(Alignment::Center)
        .block(block)
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_artwork(frame: &mut Frame<'_>, area: Rect, artwork: &mut ArtworkState) {
    let title = format!(" Cover art · {} ", artwork.protocol_name());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    artwork.render(frame, inner);
}

fn render_player(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let (title, subtitle, state) = match &app.now_playing {
        Some(item) => (
            item.title.as_str(),
            item.subtitle.as_str(),
            if app.paused { "Paused" } else { "Playing" },
        ),
        None => ("Nothing playing", "Select a track to play", "Stopped"),
    };
    let controls = if app.now_playing.is_some() {
        "Space pause/resume"
    } else {
        "Enter open/play"
    };
    let player = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                format!(" {title}"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {subtitle}"), Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" {state} "),
                Style::default().fg(Color::Black).bg(ACCENT),
            ),
            Span::styled(
                format!("  {controls}  ·  f player  ·  ? help  ·  q quit"),
                Style::default().fg(MUTED),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(format!(" s {} · n Next ", playback_order_label(app)))
            .title_style(Style::default().fg(if app.shuffle { ACCENT } else { MUTED }))
            .borders(Borders::TOP),
    );
    frame.render_widget(player, area);
}

fn render_player_focus(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    artwork: Option<&mut ArtworkState>,
) {
    let block = Block::default()
        .title(" Player focus · f/Esc return · ? help ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let horizontal = inner.width >= 80;
    let sections = Layout::default()
        .direction(if horizontal {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints(if horizontal {
            [Constraint::Percentage(62), Constraint::Percentage(38)]
        } else {
            [Constraint::Min(3), Constraint::Length(18)]
        })
        .split(inner);

    if let Some(artwork) = artwork {
        render_artwork(frame, sections[0], artwork);
    } else {
        render_empty(
            frame,
            sections[0],
            "Cover art",
            "Artwork is disabled for this session.",
        );
    }

    let item = app.now_playing.as_ref().or_else(|| app.selected());
    let (title, subtitle, playback) = match item {
        Some(item) => (
            item.title.as_str(),
            item.subtitle.as_str(),
            if app.now_playing.is_some() {
                if app.paused { "PAUSED" } else { "PLAYING" }
            } else {
                "SELECTED"
            },
        ),
        None => ("Nothing playing", "Choose a track to begin", "STOPPED"),
    };
    let details = Paragraph::new(vec![
        Line::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(subtitle, Style::default().fg(MUTED)),
        Line::default(),
        Line::styled(
            format!(" {playback} "),
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Line::default(),
        Line::styled(playback_order_label(app), Style::default().fg(ACCENT)),
        Line::styled(app.status.as_str(), Style::default().fg(MUTED)),
        Line::default(),
        Line::styled(
            "s  Toggle shuffle · n  Next",
            Style::default().fg(Color::White),
        ),
        Line::styled("Space  Pause / resume", Style::default().fg(Color::White)),
        Line::styled(
            "f / Esc  Return to browser",
            Style::default().fg(Color::White),
        ),
        Line::styled("?  Keyboard help", Style::default().fg(Color::White)),
        Line::styled("q  Quit", Style::default().fg(Color::White)),
    ])
    .alignment(if horizontal {
        Alignment::Left
    } else {
        Alignment::Center
    })
    .wrap(Wrap { trim: true })
    .block(
        Block::default()
            .title(" Now playing ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PANEL)),
    );
    frame.render_widget(details, sections[1]);
}

fn playback_order_label(app: &App) -> String {
    let mode = if app.shuffle {
        "Shuffle: on"
    } else {
        "Shuffle: off"
    };
    match app.queue.progress() {
        Some((position, total)) => format!("{mode} · Song {position}/{total}"),
        None => mode.to_owned(),
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let width = area.width.saturating_sub(4).min(76);
    let height = area.height.saturating_sub(2).min(23);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let help = Paragraph::new(vec![
        Line::styled(
            "NAVIGATION",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from("  Tab / Shift-Tab   Move between sidebar and content"),
        Line::from("  ↑ ↓ or j k        Sidebar: choose view · Content: choose track"),
        Line::from("  ← → or h l        Sidebar: enter content · Content: switch shelf"),
        Line::from("  Enter / p         Open selection or play track"),
        Line::from("  Backspace / Esc   Return from a detail view"),
        Line::default(),
        Line::styled(
            "VIEWS",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from("  g  For You     e  Explore     c  Collection     P  Playlists"),
        Line::from("  /  Search      f  Player focus"),
        Line::default(),
        Line::styled(
            "PLAYBACK",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from("  Space             Pause or resume"),
        Line::from("  s / S             Toggle displayed shuffle / reshuffle and play"),
        Line::from("  n                 Next song in the captured list"),
        Line::default(),
        Line::from("  ? / Esc / q       Close this help     q  Quit elsewhere"),
    ])
    .wrap(Wrap { trim: false })
    .block(
        Block::default()
            .title(" Help ")
            .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT)),
    );
    frame.render_widget(help, popup);
}

fn render_search(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let width = area.width.saturating_sub(8).min(72);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(5) / 3,
        width,
        height: 5,
    };
    frame.render_widget(Clear, popup);
    let search = Paragraph::new(format!("{}█", app.search_query))
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .title(" Search TIDAL · Enter submit · Esc cancel ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT)),
        );
    frame.render_widget(search, popup);
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    use super::*;

    fn rendered_text(buffer: &Buffer) -> String {
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn help_overlay_lists_discoverable_navigation() {
        let mut app = App::new(false);
        app.help_visible = true;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw succeeds");
        let text = rendered_text(terminal.backend().buffer());

        assert!(text.contains("Help"));
        assert!(text.contains("Move between sidebar and content"));
        assert!(text.contains("Player focus"));
    }

    #[test]
    fn idle_player_and_help_do_not_advertise_previews() {
        let mut app = App::new(false);
        app.help_visible = true;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw succeeds");
        let text = rendered_text(terminal.backend().buffer());
        assert!(text.contains("Select a track to play"));
        assert!(text.contains("Open selection or play track"));
        assert!(!text.contains("preview"));
    }

    #[test]
    fn player_focus_uses_the_full_view() {
        let mut app = App::new(false);
        app.player_focused = true;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw succeeds");
        let text = rendered_text(terminal.backend().buffer());

        assert!(text.contains("Player focus"));
        assert!(text.contains("Cover art"));
        assert!(text.contains("Now playing"));
    }

    #[test]
    fn player_labels_match_playback_and_pause_states() {
        let mut app = App::new(true);
        app.playback_started(crate::models::MediaItem::new(
            "1",
            "Track",
            "Artist",
            crate::models::MediaKind::Track,
        ));
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw succeeds");
        let text = rendered_text(terminal.backend().buffer());
        assert!(text.contains("Playing"));
        assert!(!text.contains("Preview"));

        app.player_focused = true;
        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw succeeds");
        let text = rendered_text(terminal.backend().buffer());
        assert!(text.contains("PLAYING"));
        assert!(!text.contains("PREVIEW"));

        app.paused = true;
        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw succeeds");
        let text = rendered_text(terminal.backend().buffer());
        assert!(text.contains("PAUSED"));
        assert!(!text.contains("PREVIEW"));
    }

    #[test]
    fn shuffle_status_is_visible_in_browser_and_player_focus() {
        for width in [60, 100] {
            let mut app = App::new(true);
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).expect("test terminal");
            terminal
                .draw(|frame| draw(frame, &app, None))
                .expect("draw");
            assert!(rendered_text(terminal.backend().buffer()).contains("Shuffle: off"));
            app.shuffle = true;
            for focused in [false, true] {
                app.player_focused = focused;
                terminal
                    .draw(|frame| draw(frame, &app, None))
                    .expect("draw");
                assert!(rendered_text(terminal.backend().buffer()).contains("Shuffle: on"));
            }
        }
    }

    #[test]
    fn help_advertises_shuffle_and_next_song() {
        let mut app = App::new(true);
        app.help_visible = true;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw");
        let text = rendered_text(terminal.backend().buffer());
        assert!(text.contains("Toggle displayed shuffle / reshuffle and play"));
        assert!(text.contains("Next song in the captured list"));
    }

    #[test]
    fn s_shows_the_shuffled_song_order_and_highlights_the_song_that_will_play() {
        use crate::app::Action;
        use crate::models::{MediaItem, MediaKind, Shelf};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(true);
        app.replace_shelves(vec![Shelf::new(
            "Playlist",
            (0..6)
                .map(|index| {
                    MediaItem::new(
                        index.to_string(),
                        format!("Song {index}"),
                        "Artist",
                        MediaKind::Track,
                    )
                })
                .collect(),
        )]);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw");
        let text = rendered_text(terminal.backend().buffer());
        assert!(text.contains("shuffled song order"));
        assert!(text.contains("Enter plays highlighted song"));
        let positions: Vec<_> = app.shelves[0]
            .items
            .iter()
            .map(|item| text.find(&item.title).expect("song rendered"))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let selected = app.selected().unwrap().clone();
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
            Action::Play(selected)
        );
    }

    #[test]
    fn loaded_shelves_render_outside_for_you() {
        let mut app = App::new(false);
        app.screen = Screen::Collection;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app, None))
            .expect("draw succeeds");
        let text = rendered_text(terminal.backend().buffer());

        assert!(text.contains("Custom mixes"));
    }
}
