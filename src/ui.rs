use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, symbols};

use crate::app::{App, Screen};
use crate::artwork::ArtworkState;

const ACCENT: Color = Color::Rgb(0, 230, 205);
const PANEL: Color = Color::Rgb(24, 24, 24);
const MUTED: Color = Color::Rgb(145, 145, 145);

pub fn draw(frame: &mut Frame<'_>, app: &App, artwork: Option<&mut ArtworkState>) {
    let area = frame.area();
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
    let list = List::new(items).block(
        Block::default()
            .title(" tidalbar ")
            .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_set(symbols::border::PLAIN)
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
        Span::styled("   / Search", Style::default().fg(MUTED)),
    ]))
    .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(heading, content[0]);

    match app.screen {
        Screen::ForYou => render_shelves(frame, content[1], app),
        Screen::Explore => render_empty(
            frame,
            content[1],
            "Explore",
            "Genres, moods, staff picks, and new releases will appear here.",
        ),
        Screen::Collection => render_empty(
            frame,
            content[1],
            "Collection",
            "Liked tracks, albums, artists, and videos will appear here.",
        ),
        Screen::Playlists => render_empty(
            frame,
            content[1],
            "Playlists",
            "Your playlists and folders will appear here.",
        ),
    }

    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::default().fg(MUTED)),
        content[2],
    );
}

fn render_shelves(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.shelves.is_empty() {
        render_empty(frame, area, "For You", "No recommendations are available.");
        return;
    }

    let visible = usize::min(app.shelves.len(), usize::from(area.height / 4).max(1));
    let constraints = vec![Constraint::Length(4); visible];
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let first = app.selected_shelf.saturating_sub(visible.saturating_sub(1));
    for (row, shelf_index) in rows.iter().zip(first..app.shelves.len()) {
        let Some(shelf) = app.shelves.get(shelf_index) else {
            break;
        };
        let selected_shelf = shelf_index == app.selected_shelf;
        let mut title_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        if selected_shelf {
            title_style = title_style.fg(ACCENT);
        }

        let mut item_spans = Vec::new();
        for (item_index, item) in shelf.items.iter().enumerate() {
            let selected = selected_shelf && item_index == app.selected_item;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(PANEL)
            };
            item_spans.push(Span::styled(
                format!(" {} · {} ", item.title, item.subtitle),
                style,
            ));
            item_spans.push(Span::raw(" "));
        }

        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(format!(" {}", shelf.title), title_style),
                Line::from(item_spans),
            ])
            .wrap(Wrap { trim: true }),
            *row,
        );
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
            if app.paused { "Paused" } else { "Preview" },
        ),
        None => ("Nothing playing", "Select an official preview", "Stopped"),
    };
    let controls = if app.now_playing.is_some() {
        "Space pause/resume"
    } else {
        "Enter play preview"
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
                format!("  {controls}  ·  hjkl navigate  ·  q quit"),
                Style::default().fg(MUTED),
            ),
        ]),
    ])
    .block(Block::default().borders(Borders::TOP));
    frame.render_widget(player, area);
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
