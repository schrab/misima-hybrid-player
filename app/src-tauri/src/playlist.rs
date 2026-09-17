use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub id: u64,
    pub path: String,
    pub title: String,
}

#[derive(Debug, Default)]
pub struct Playlist {
    entries: Vec<PlaylistEntry>,
    next_id: u64,
    pub current: Option<usize>,
}

impl Playlist {
    pub fn add_paths(&mut self, paths: &[String]) {
        for p in paths {
            let path = Path::new(p);
            let title = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| p.clone());
            self.entries.push(PlaylistEntry {
                id: self.next_id,
                path: p.clone(),
                title,
            });
            self.next_id += 1;
        }
    }

    pub fn entries(&self) -> &[PlaylistEntry] {
        &self.entries
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.entries.len() || to >= self.entries.len() || from == to {
            return;
        }
        let item = self.entries.remove(from);
        self.entries.insert(to, item);
        if let Some(c) = self.current {
            if c == from {
                self.current = Some(to);
            } else if from < c && to >= c {
                self.current = Some(c - 1);
            } else if from > c && to <= c {
                self.current = Some(c + 1);
            }
        }
    }

    pub fn set_current(&mut self, index: Option<usize>) {
        if matches!(index, Some(i) if i < self.entries.len()) {
            self.current = index;
        } else {
            self.current = None;
        }
    }

    pub fn current_path(&self) -> Option<&str> {
        self.current
            .and_then(|i| self.entries.get(i))
            .map(|e| e.path.as_str())
    }

    pub fn current_id(&self) -> Option<u64> {
        self.current.and_then(|i| self.entries.get(i)).map(|e| e.id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_updates_current() {
        let mut p = Playlist::default();
        p.add_paths(&["a.mp3".into(), "b.mp3".into(), "c.mp3".into()]);
        p.set_current(Some(2));
        p.reorder(2, 0);
        assert_eq!(p.entries()[0].path, "c.mp3");
        assert_eq!(p.current, Some(0));
    }

    #[test]
    fn titles_from_filenames() {
        let mut p = Playlist::default();
        p.add_paths(&["/music/Track 01.flac".into()]);
        assert_eq!(p.entries()[0].title, "Track 01.flac");
    }
}
