//! Track permutations for a visible shelf, retaining occurrence identities so
//! turning shuffle off restores even playlists containing duplicate songs.

use rand::{Rng, seq::SliceRandom};

use crate::models::{MediaItem, MediaKind, Shelf};

#[derive(Clone, Debug)]
pub(crate) struct SongListOrder {
    original_positions: Vec<usize>,
    shuffled: bool,
}

impl SongListOrder {
    pub fn new(shelf: &Shelf) -> Self {
        Self {
            original_positions: (0..shelf.items.len()).collect(),
            shuffled: false,
        }
    }

    pub fn original_position(&self, displayed: usize) -> Option<usize> {
        self.original_positions.get(displayed).copied()
    }

    pub fn displayed_position(&self, original: usize) -> Option<usize> {
        self.original_positions
            .iter()
            .position(|position| *position == original)
    }

    pub fn set_shuffle(&mut self, shelf: &mut Shelf, shuffle: bool, rng: &mut impl Rng) {
        if shuffle == self.shuffled {
            return;
        }
        if shuffle {
            self.reshuffle(shelf, rng);
        } else {
            self.restore(shelf);
        }
    }

    pub fn reshuffle(&mut self, shelf: &mut Shelf, rng: &mut impl Rng) {
        let mut order: Vec<_> = (0..shelf
            .items
            .iter()
            .filter(|item| item.kind == MediaKind::Track)
            .count())
            .collect();
        order.shuffle(rng);
        self.set_track_order(shelf, &order);
    }

    /// Reflect an existing queue without changing that queue while browsing.
    /// Songs outside a sequential queue are appended in original order.
    pub fn set_track_order(&mut self, shelf: &mut Shelf, order: &[usize]) {
        self.restore(shelf);
        let slots: Vec<_> = shelf
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.kind == MediaKind::Track)
            .map(|(index, _)| index)
            .collect();
        let tracks: Vec<_> = slots
            .iter()
            .map(|index| (*index, shelf.items[*index].clone()))
            .collect();
        let mut included = vec![false; tracks.len()];
        for rank in order {
            included[*rank] = true;
        }
        let complete_order = order
            .iter()
            .copied()
            .chain((0..tracks.len()).filter(|index| !included[*index]));
        for (slot, rank) in slots.into_iter().zip(complete_order) {
            let (original, track) = &tracks[rank];
            shelf.items[slot] = track.clone();
            self.original_positions[slot] = *original;
        }
        self.shuffled = true;
    }

    fn restore(&mut self, shelf: &mut Shelf) {
        let mut entries: Vec<_> = self
            .original_positions
            .iter()
            .copied()
            .zip(std::mem::take(&mut shelf.items))
            .collect();
        entries.sort_by_key(|(original, _)| *original);
        shelf.items = entries.into_iter().map(|(_, item)| item).collect();
        self.original_positions = (0..shelf.items.len()).collect();
        self.shuffled = false;
    }

    /// Canonical tracks and their displayed permutation. Queue indices use the
    /// canonical order, so shuffle-off restores the original unplayed order.
    pub fn queue_plan(&self, shelf: &Shelf) -> (Vec<MediaItem>, Vec<usize>) {
        let displayed: Vec<_> = self
            .original_positions
            .iter()
            .copied()
            .zip(shelf.items.iter())
            .filter(|(_, item)| item.kind == MediaKind::Track)
            .collect();
        let mut canonical = displayed.clone();
        canonical.sort_by_key(|(original, _)| *original);
        let order = displayed
            .iter()
            .map(|(original, _)| {
                canonical
                    .binary_search_by_key(original, |(position, _)| *position)
                    .expect("each displayed occurrence has a canonical position")
            })
            .collect();
        (
            canonical
                .into_iter()
                .map(|(_, item)| item.clone())
                .collect(),
            order,
        )
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    #[test]
    fn shuffle_preserves_non_tracks_and_restores_duplicate_occurrences() {
        let song = MediaItem::new("same", "Song", "Artist", MediaKind::Track);
        let album = MediaItem::new("album", "Album", "Artist", MediaKind::Album);
        let mut shelf = Shelf::new(
            "Mixed",
            vec![
                song.clone(),
                album.clone(),
                song,
                MediaItem::new("last", "Last", "Artist", MediaKind::Track),
            ],
        );
        let original = shelf.clone();
        let mut order = SongListOrder::new(&shelf);
        order.set_shuffle(&mut shelf, true, &mut StdRng::seed_from_u64(42));
        assert_eq!(shelf.items[1], album);
        let (tracks, displayed_order) = order.queue_plan(&shelf);
        assert_eq!(
            tracks
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["same", "same", "last"]
        );
        assert_ne!(displayed_order, [0, 1, 2]);
        let displayed_tracks: Vec<_> = shelf
            .items
            .iter()
            .filter(|item| item.kind == MediaKind::Track)
            .cloned()
            .collect();
        assert_eq!(
            displayed_order
                .iter()
                .map(|index| tracks[*index].clone())
                .collect::<Vec<_>>(),
            displayed_tracks
        );
        order.set_shuffle(&mut shelf, false, &mut StdRng::seed_from_u64(1));
        assert_eq!(shelf, original);
        assert_eq!(order.original_positions, [0, 1, 2, 3]);
    }
}
