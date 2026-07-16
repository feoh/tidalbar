#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MediaKind {
    Album,
    Artist,
    Mix,
    Playlist,
    Radio,
    Track,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: MediaKind,
    pub artwork_url: Option<String>,
    pub preview_url: Option<String>,
}

impl MediaItem {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        subtitle: impl Into<String>,
        kind: MediaKind,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            kind,
            artwork_url: None,
            preview_url: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shelf {
    pub title: String,
    pub items: Vec<MediaItem>,
}

impl Shelf {
    pub fn new(title: impl Into<String>, items: Vec<MediaItem>) -> Self {
        Self {
            title: title.into(),
            items,
        }
    }
}

pub fn demo_home() -> Vec<Shelf> {
    vec![
        Shelf::new(
            "Custom mixes",
            vec![
                MediaItem::new(
                    "daily",
                    "My Daily Discovery",
                    "Made for you",
                    MediaKind::Mix,
                ),
                MediaItem::new(
                    "mix-1",
                    "My Mix 1",
                    "Experimental electronic",
                    MediaKind::Mix,
                ),
                MediaItem::new("mix-2", "My Mix 2", "Modern bass music", MediaKind::Mix),
                MediaItem::new("mix-3", "My Mix 3", "2010s dance-pop", MediaKind::Mix),
                MediaItem::new("mix-4", "My Mix 4", "Drum & bass", MediaKind::Mix),
            ],
        ),
        Shelf::new(
            "Personal radio stations",
            vec![
                MediaItem::new(
                    "radio-1",
                    "Track Radio",
                    "Delightful Universe",
                    MediaKind::Radio,
                ),
                MediaItem::new("radio-2", "Artist Radio", "Bonobo", MediaKind::Radio),
                MediaItem::new("radio-3", "Track Radio", "Cyan", MediaKind::Radio),
                MediaItem::new("radio-4", "Artist Radio", "Joker", MediaKind::Radio),
            ],
        ),
        Shelf::new(
            "Recently played",
            vec![
                MediaItem::new(
                    "recent-1",
                    "Promises",
                    "Floating Points & Pharoah Sanders",
                    MediaKind::Album,
                ),
                MediaItem::new(
                    "recent-2",
                    "R Plus Seven",
                    "Oneohtrix Point Never",
                    MediaKind::Album,
                ),
                MediaItem::new("recent-3", "Teponaztli Dub", "Puchoc", MediaKind::Track),
            ],
        ),
        Shelf::new(
            "Your playlists",
            vec![
                MediaItem::new("playlist-1", "Rain Sounds", "TIDAL", MediaKind::Playlist),
                MediaItem::new(
                    "playlist-2",
                    "Electronic: RISING",
                    "TIDAL",
                    MediaKind::Playlist,
                ),
                MediaItem::new("playlist-3", "Dub 101", "TIDAL", MediaKind::Playlist),
            ],
        ),
    ]
}
