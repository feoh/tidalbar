//! Playback order for a snapshot of a song list, independent of browsing state.

use rand::{Rng, seq::SliceRandom};

use crate::models::MediaItem;

#[derive(Debug, Default)]
pub struct PlaybackQueue {
    tracks: Vec<MediaItem>,
    order: Vec<usize>,
    position: Option<usize>,
}

impl PlaybackQueue {
    /// A selected song plays first. Without a selection, shuffle the whole list.
    pub fn start(
        &mut self,
        tracks: Vec<MediaItem>,
        selected: Option<usize>,
        shuffle: bool,
        rng: &mut impl Rng,
    ) -> Option<MediaItem> {
        self.tracks = tracks;
        self.order = match selected.filter(|index| *index < self.tracks.len()) {
            Some(index) if shuffle => {
                let mut rest: Vec<_> = (0..self.tracks.len()).filter(|i| *i != index).collect();
                rest.shuffle(rng);
                std::iter::once(index).chain(rest).collect()
            }
            Some(index) => (index..self.tracks.len()).collect(),
            None => {
                let mut order: Vec<_> = (0..self.tracks.len()).collect();
                if shuffle {
                    order.shuffle(rng);
                }
                order
            }
        };
        self.position = (!self.order.is_empty()).then_some(0);
        self.current().cloned()
    }

    /// Start in the displayed order without randomizing it a second time.
    /// In shuffle mode, wrap after the last row to include earlier occurrences.
    pub fn start_ordered(
        &mut self,
        tracks: Vec<MediaItem>,
        displayed_order: Vec<usize>,
        selected: usize,
        wrap: bool,
    ) -> Option<MediaItem> {
        let selected = selected.min(displayed_order.len());
        self.tracks = tracks;
        self.order = displayed_order[selected..]
            .iter()
            .copied()
            .chain(displayed_order[..selected].iter().copied().filter(|_| wrap))
            .collect();
        self.position = (!self.order.is_empty()).then_some(0);
        self.current().cloned()
    }

    pub(crate) fn visible_order(&self) -> Option<(&[usize], usize)> {
        self.position
            .map(|position| (self.order.as_slice(), position))
    }

    pub fn matches_tracks(&self, tracks: &[MediaItem]) -> bool {
        self.position.is_some() && self.tracks == tracks
    }

    /// Align the unplayed remainder with the visible list, preserving the
    /// current song and all previously played/skipped occurrences.
    pub fn set_display_order(&mut self, displayed_order: &[usize]) {
        let Some(position) = self.position else {
            return;
        };
        let mut ranks = vec![usize::MAX; self.tracks.len()];
        for (rank, index) in displayed_order.iter().copied().enumerate() {
            if let Some(slot) = ranks.get_mut(index) {
                *slot = rank;
            }
        }
        self.order[position + 1..].sort_by_key(|index| ranks[*index]);
    }

    /// Reorder only the unplayed remainder; never repeat already played entries.
    pub fn set_shuffle(&mut self, shuffle: bool, rng: &mut impl Rng) {
        let Some(position) = self.position else {
            return;
        };
        let remaining = &mut self.order[position + 1..];
        if shuffle {
            remaining.shuffle(rng);
        } else {
            remaining.sort_unstable();
        }
    }

    pub fn advance(&mut self) -> Option<MediaItem> {
        self.position = self
            .position
            .and_then(|position| (position + 1 < self.order.len()).then_some(position + 1));
        self.current().cloned()
    }

    pub fn progress(&self) -> Option<(usize, usize)> {
        self.position
            .map(|position| (position + 1, self.order.len()))
    }

    fn current(&self) -> Option<&MediaItem> {
        self.position
            .and_then(|position| self.order.get(position))
            .and_then(|index| self.tracks.get(*index))
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use crate::models::MediaKind;

    use super::*;

    fn tracks() -> Vec<MediaItem> {
        (0..12)
            .map(|index| MediaItem::new(index.to_string(), "Song", "Artist", MediaKind::Track))
            .collect()
    }

    fn drain(queue: &mut PlaybackQueue) -> Vec<String> {
        let mut ids = Vec::new();
        while let Some(track) = queue.advance() {
            ids.push(track.id);
        }
        ids
    }

    #[test]
    fn shuffled_queue_starts_at_selection_and_visits_every_entry_once() {
        let mut queue = PlaybackQueue::default();
        let mut rng = StdRng::seed_from_u64(42);
        let first = queue.start(tracks(), Some(4), true, &mut rng).unwrap();
        assert_eq!(first.id, "4");
        assert_eq!(queue.progress(), Some((1, 12)));
        let mut ids = vec![first.id];
        ids.extend(drain(&mut queue));
        let unshuffled: Vec<_> = std::iter::once(4)
            .chain((0..12).filter(|i| *i != 4))
            .map(|i| i.to_string())
            .collect();
        assert_ne!(ids, unshuffled, "the remainder must actually be shuffled");
        ids.sort();
        let mut expected: Vec<_> = tracks().into_iter().map(|track| track.id).collect();
        expected.sort();
        assert_eq!(ids, expected);
        assert_eq!(queue.progress(), None);
        assert_eq!(queue.advance(), None);
    }

    #[test]
    fn sequential_playback_runs_from_selection_to_end() {
        let mut queue = PlaybackQueue::default();
        let first = queue.start(tracks(), Some(9), false, &mut StdRng::seed_from_u64(1));
        assert_eq!(first.unwrap().id, "9");
        assert_eq!(drain(&mut queue), ["10", "11"]);
    }

    #[test]
    fn toggling_preserves_current_and_played_songs_and_restores_list_order() {
        let mut queue = PlaybackQueue::default();
        let mut rng = StdRng::seed_from_u64(42);
        queue.start(tracks(), Some(0), false, &mut rng);
        queue.advance();
        queue.set_shuffle(true, &mut rng);
        assert_eq!(queue.current().unwrap().id, "1");
        let played = queue.advance().unwrap().id;
        queue.set_shuffle(false, &mut rng);
        assert_eq!(queue.current().unwrap().id, played);
        let remainder = drain(&mut queue);
        let expected: Vec<_> = (2..12)
            .map(|i| i.to_string())
            .filter(|id| *id != played)
            .collect();
        assert_eq!(remainder, expected);
    }

    #[test]
    fn empty_single_and_duplicate_entries_are_safe() {
        let mut queue = PlaybackQueue::default();
        let mut rng = StdRng::seed_from_u64(42);
        assert_eq!(queue.start(vec![], None, true, &mut rng), None);
        queue.set_shuffle(false, &mut rng);
        assert_eq!(queue.advance(), None);
        let track = tracks().remove(0);
        assert_eq!(
            queue.start(vec![track.clone()], None, true, &mut rng),
            Some(track.clone())
        );
        assert_eq!(queue.advance(), None);
        queue.start(vec![track.clone(), track.clone()], None, true, &mut rng);
        assert_eq!(queue.advance(), Some(track));
        assert_eq!(queue.advance(), None);
    }

    #[test]
    fn shuffle_list_randomizes_the_first_song_as_well() {
        let mut first_ids = std::collections::HashSet::new();
        for seed in 0..16 {
            let mut queue = PlaybackQueue::default();
            first_ids.insert(
                queue
                    .start(tracks(), None, true, &mut StdRng::seed_from_u64(seed))
                    .unwrap()
                    .id,
            );
        }
        assert!(first_ids.len() > 1);
    }
}
