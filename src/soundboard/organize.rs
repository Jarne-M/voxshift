//! Soundboard organisation: folders, the built-in tabs (All, Favorites, Recent, Unsorted),
//! search and sorting. Pure bookkeeping on the saved clip entries, so it is tested here
//! without any UI.
//!
//! A clip lives in exactly one folder (or none: "Unsorted") and always shows under "All".
//! Clips are referred to by their saved `uid`, which never changes, so folders, favourites
//! and recents survive restarts and reordering.

use super::library::SoundEntry;
use serde::{Deserialize, Serialize};

/// How many clips "Recent" remembers.
pub const RECENT_MAX: usize = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardTab {
    #[default]
    All,
    Favorites,
    Recent,
    Unsorted,
    Folder(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    /// Your own order (drag a clip onto another to move it there).
    #[default]
    Manual,
    Name,
    MostPlayed,
    RecentlyAdded,
}

impl SortMode {
    pub const ALL: [SortMode; 4] = [
        SortMode::Manual,
        SortMode::Name,
        SortMode::MostPlayed,
        SortMode::RecentlyAdded,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Manual => "My order",
            SortMode::Name => "Name",
            SortMode::MostPlayed => "Most played",
            SortMode::RecentlyAdded => "Recently added",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: u32,
    pub name: String,
}

/// How the clips are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardView {
    /// Pads in a grid.
    #[default]
    Grid,
    /// One compact row per clip.
    List,
}

/// The width of a pad, in points.
pub const PAD_SIZE_DEFAULT: f32 = 150.0;
pub const PAD_SIZE_RANGE: std::ops::RangeInclusive<f32> = 110.0..=260.0;

/// Saved in the config.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardSettings {
    /// In tab order.
    pub folders: Vec<Folder>,
    /// Clip uids, most recently played first.
    pub recent: Vec<u64>,
    /// The tab that was open last.
    pub tab: BoardTab,
    pub sort: SortMode,
    pub view: BoardView,
    /// Pad width in the grid view.
    pub pad_size: f32,
    /// Master volume of every clip (0..1), on top of each clip's own volume.
    pub master_volume: f32,
}

impl Default for BoardSettings {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            recent: Vec::new(),
            tab: BoardTab::default(),
            sort: SortMode::default(),
            view: BoardView::default(),
            pad_size: PAD_SIZE_DEFAULT,
            master_volume: 1.0,
        }
    }
}

impl BoardSettings {
    /// Puts a deleted folder back at `index` (undo). If its id was given to a folder made
    /// in the meantime it gets a new one. Returns the id it has now.
    pub fn restore_folder(&mut self, mut folder: Folder, index: usize) -> u32 {
        if self.folders.iter().any(|f| f.id == folder.id) {
            folder.id = self.folders.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        }
        folder.name = self.unique_name(&folder.name, None);
        let id = folder.id;
        self.folders.insert(index.min(self.folders.len()), folder);
        id
    }

    /// A new folder (the name is made unique: "Memes", "Memes (2)", ...). Returns its id.
    pub fn create_folder(&mut self, name: &str) -> u32 {
        let id = self.folders.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        let name = self.unique_name(name.trim(), None);
        self.folders.push(Folder { id, name });
        id
    }

    pub fn rename_folder(&mut self, id: u32, name: &str) -> bool {
        let name = self.unique_name(name.trim(), Some(id));
        match self.folders.iter_mut().find(|f| f.id == id) {
            Some(f) => {
                f.name = name;
                true
            }
            None => false,
        }
    }

    /// Removes a folder. Its clips go to "Unsorted"; no clip or file is ever deleted.
    pub fn delete_folder(&mut self, id: u32, sounds: &mut [SoundEntry]) {
        self.folders.retain(|f| f.id != id);
        for s in sounds.iter_mut().filter(|s| s.folder == Some(id)) {
            s.folder = None;
        }
        if self.tab == BoardTab::Folder(id) {
            self.tab = BoardTab::Unsorted;
        }
    }

    /// Moves a folder one place left (-1) or right (+1) among the tabs.
    pub fn move_folder(&mut self, id: u32, delta: isize) {
        let Some(i) = self.folders.iter().position(|f| f.id == id) else {
            return;
        };
        let j = (i as isize + delta).clamp(0, self.folders.len() as isize - 1) as usize;
        let f = self.folders.remove(i);
        self.folders.insert(j, f);
    }

    pub fn folder_name(&self, id: u32) -> Option<&str> {
        self.folders
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.name.as_str())
    }

    fn unique_name(&self, wanted: &str, except: Option<u32>) -> String {
        let base = if wanted.is_empty() {
            "New folder"
        } else {
            wanted
        };
        let taken = |n: &str| {
            self.folders
                .iter()
                .any(|f| Some(f.id) != except && f.name.eq_ignore_ascii_case(n))
        };
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} ({i})"))
            .find(|n| !taken(n))
            .expect("a free number")
    }

    /// Forgets folders that no longer exist on clips loaded from an old or edited config.
    pub fn repair(&mut self, sounds: &mut [SoundEntry]) {
        for s in sounds.iter_mut() {
            if let Some(f) = s.folder
                && self.folder_name(f).is_none()
            {
                s.folder = None;
            }
        }
        if let BoardTab::Folder(f) = self.tab
            && self.folder_name(f).is_none()
        {
            self.tab = BoardTab::All;
        }
        let uids: Vec<u64> = sounds.iter().map(|s| s.uid).collect();
        self.recent.retain(|u| uids.contains(u));
        self.recent.truncate(RECENT_MAX);
        if !self.pad_size.is_finite() {
            self.pad_size = PAD_SIZE_DEFAULT;
        }
        self.pad_size = self
            .pad_size
            .clamp(*PAD_SIZE_RANGE.start(), *PAD_SIZE_RANGE.end());
    }
}

/// Puts `uid` at the front of `recent` (once), keeping at most [`RECENT_MAX`].
pub fn push_recent(recent: &mut Vec<u64>, uid: u64) {
    recent.retain(|&u| u != uid);
    recent.insert(0, uid);
    recent.truncate(RECENT_MAX);
}

/// Lower case without accents, for searching: "Écoute ÇA" -> "ecoute ca".
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'æ' => "ae",
            'ç' | 'ć' | 'č' => "c",
            'ď' | 'đ' => "d",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'ğ' => "g",
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
            'ł' | 'ľ' | 'ĺ' => "l",
            'ñ' | 'ń' | 'ň' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
            'œ' => "oe",
            'ř' | 'ŕ' => "r",
            'ś' | 'š' | 'ş' => "s",
            'ß' => "ss",
            'ť' | 'ţ' => "t",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
            'ý' | 'ÿ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(plain);
    }
    out
}

/// True if `name` contains `query` (partial, case- and accent-insensitive).
pub fn matches(name: &str, query: &str) -> bool {
    let q = fold(query.trim());
    q.is_empty() || fold(name).contains(&q)
}

/// The clips to show (indexes into `sounds`), in display order. While searching, every
/// folder is searched, whatever tab is open.
pub fn visible(sounds: &[SoundEntry], s: &BoardSettings, search: &str) -> Vec<usize> {
    let searching = !search.trim().is_empty();
    if s.tab == BoardTab::Recent && !searching {
        // Recent has its own order: most recent first.
        return s
            .recent
            .iter()
            .filter_map(|u| sounds.iter().position(|e| e.uid == *u))
            .collect();
    }
    let mut out: Vec<usize> = (0..sounds.len())
        .filter(|&i| {
            let e = &sounds[i];
            if searching {
                return matches(&e.name, search);
            }
            match s.tab {
                BoardTab::All => true,
                BoardTab::Favorites => e.favorite,
                BoardTab::Unsorted => e.folder.is_none(),
                BoardTab::Folder(f) => e.folder == Some(f),
                BoardTab::Recent => unreachable!("handled above"),
            }
        })
        .collect();
    match s.sort {
        SortMode::Manual => {}
        SortMode::Name => out.sort_by(|&a, &b| fold(&sounds[a].name).cmp(&fold(&sounds[b].name))),
        SortMode::MostPlayed => out.sort_by(|&a, &b| sounds[b].plays.cmp(&sounds[a].plays)),
        SortMode::RecentlyAdded => out.sort_by(|&a, &b| sounds[b].uid.cmp(&sounds[a].uid)),
    }
    out
}

/// The order of all clips under `sort`, as indices into `sounds` (what "All" shows).
pub fn sorted_order(sounds: &[SoundEntry], sort: SortMode) -> Vec<usize> {
    let all = BoardSettings {
        tab: BoardTab::All,
        sort,
        ..Default::default()
    };
    visible(sounds, &all, "")
}

/// Rearranges `items` into `order` (a permutation of its indices, e.g. from `sorted_order`).
pub fn apply_order<T>(items: &mut Vec<T>, order: &[usize]) {
    if order.len() != items.len() {
        return;
    }
    let mut old: Vec<Option<T>> = std::mem::take(items).into_iter().map(Some).collect();
    items.extend(
        order
            .iter()
            .filter_map(|&i| old.get_mut(i).and_then(Option::take)),
    );
}

/// Moves the clip at `from` so it sits where the clip at `to` is (manual order).
pub fn reorder<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from == to || from >= items.len() || to >= items.len() {
        return;
    }
    let item = items.remove(from);
    items.insert(to, item);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(uid: u64, name: &str) -> SoundEntry {
        SoundEntry {
            uid,
            ..SoundEntry::from_path(format!("C:/s/{name}.wav").into())
        }
    }

    fn names(sounds: &[SoundEntry], idx: &[usize]) -> Vec<String> {
        idx.iter().map(|&i| sounds[i].name.clone()).collect()
    }

    #[test]
    fn folders_are_created_renamed_reordered_and_deleted_without_losing_clips() {
        let mut s = BoardSettings::default();
        let memes = s.create_folder("Memes");
        let music = s.create_folder("Music");
        let memes2 = s.create_folder("memes");
        assert_eq!(
            s.folder_name(memes2),
            Some("memes (2)"),
            "names stay unique"
        );
        assert!(s.rename_folder(memes2, "Stingers"));
        assert!(!s.rename_folder(99, "x"));
        assert!(
            s.rename_folder(music, "Memes"),
            "renaming into a taken name"
        );
        assert_eq!(s.folder_name(music), Some("Memes (2)"));
        assert_eq!(s.create_folder("  "), 4);
        assert_eq!(s.folder_name(4), Some("New folder"));

        s.move_folder(memes2, -1);
        let order: Vec<u32> = s.folders.iter().map(|f| f.id).collect();
        assert_eq!(order, [memes, memes2, music, 4]);
        s.move_folder(memes, -1); // already first: stays
        s.move_folder(4, 5); // clamped to the end
        assert_eq!(s.folders[0].id, memes);

        let mut sounds = vec![clip(1, "airhorn"), clip(2, "bruh"), clip(3, "wow")];
        sounds[0].folder = Some(memes);
        sounds[1].folder = Some(memes);
        sounds[2].folder = Some(music);
        s.tab = BoardTab::Folder(memes);
        s.delete_folder(memes, &mut sounds);
        assert_eq!(sounds.len(), 3, "no clip is deleted");
        assert_eq!(sounds[0].folder, None, "to Unsorted");
        assert_eq!(sounds[2].folder, Some(music), "others untouched");
        assert_eq!(s.tab, BoardTab::Unsorted, "the open tab moves along");
        assert!(s.folder_name(memes).is_none());
    }

    #[test]
    fn tabs_show_the_right_clips_and_all_shows_every_clip() {
        let mut s = BoardSettings::default();
        let f = s.create_folder("Memes");
        let mut sounds = vec![clip(1, "airhorn"), clip(2, "bruh"), clip(3, "wow")];
        sounds[1].folder = Some(f); // moving a clip is just setting its folder
        sounds[2].favorite = true;
        let show = |tab, sounds: &[SoundEntry], s: &mut BoardSettings| {
            s.tab = tab;
            names(sounds, &visible(sounds, s, ""))
        };
        assert_eq!(
            show(BoardTab::All, &sounds, &mut s),
            ["airhorn", "bruh", "wow"]
        );
        assert_eq!(show(BoardTab::Folder(f), &sounds, &mut s), ["bruh"]);
        assert_eq!(
            show(BoardTab::Unsorted, &sounds, &mut s),
            ["airhorn", "wow"]
        );
        assert_eq!(show(BoardTab::Favorites, &sounds, &mut s), ["wow"]);
        sounds[1].folder = None; // moved back to Unsorted
        assert_eq!(
            show(BoardTab::Folder(f), &sounds, &mut s),
            Vec::<String>::new()
        );
    }

    #[test]
    fn search_is_partial_case_and_accent_insensitive_across_all_folders() {
        let mut s = BoardSettings::default();
        let f = s.create_folder("French");
        let mut sounds = vec![
            clip(1, "Écoute ça"),
            clip(2, "AIRHORN remix"),
            clip(3, "Smörgåsbord"),
            clip(4, "bruh"),
        ];
        sounds[0].folder = Some(f);
        s.tab = BoardTab::Favorites; // nothing is a favourite, but search covers everything
        assert_eq!(names(&sounds, &visible(&sounds, &s, "ecou")), ["Écoute ça"]);
        assert_eq!(
            names(&sounds, &visible(&sounds, &s, "COUTE CA")),
            ["Écoute ça"]
        );
        assert_eq!(
            names(&sounds, &visible(&sounds, &s, "horn")),
            ["AIRHORN remix"]
        );
        assert_eq!(
            names(&sounds, &visible(&sounds, &s, "smorgas")),
            ["Smörgåsbord"]
        );
        assert!(visible(&sounds, &s, "zzz").is_empty());
        assert_eq!(
            visible(&sounds, &s, "   ").len(),
            0,
            "blank search = the tab (no favourites)"
        );
        assert_eq!(fold("Œuvre ÆGIS straße"), "oeuvre aegis strasse");
        assert!(matches("Ça va", "CA V"));
    }

    #[test]
    fn recents_are_newest_first_unique_and_capped_at_twenty() {
        let mut r = Vec::new();
        for uid in 1..=25 {
            push_recent(&mut r, uid);
        }
        assert_eq!(r.len(), RECENT_MAX);
        assert_eq!(r[0], 25);
        assert_eq!(*r.last().unwrap(), 6);
        push_recent(&mut r, 10); // played again: moves to the front, not doubled
        assert_eq!(r[0], 10);
        assert_eq!(r.iter().filter(|&&u| u == 10).count(), 1);
        assert_eq!(r.len(), RECENT_MAX);

        // The Recent tab shows them in that order, skipping clips that were removed.
        let sounds = vec![
            clip(10, "ten"),
            clip(25, "twenty-five"),
            clip(24, "twenty-four"),
        ];
        let s = BoardSettings {
            recent: r,
            tab: BoardTab::Recent,
            ..Default::default()
        };
        assert_eq!(
            names(&sounds, &visible(&sounds, &s, "")),
            ["ten", "twenty-five", "twenty-four"]
        );
    }

    #[test]
    fn sorting_and_manual_reordering() {
        let mut sounds = vec![clip(1, "charlie"), clip(2, "Alpha"), clip(3, "bravo")];
        sounds[0].plays = 5;
        sounds[2].plays = 9;
        let mut s = BoardSettings::default();
        let order =
            |s: &BoardSettings, sounds: &[SoundEntry]| names(sounds, &visible(sounds, s, ""));
        assert_eq!(
            order(&s, &sounds),
            ["charlie", "Alpha", "bravo"],
            "manual = list order"
        );
        s.sort = SortMode::Name;
        assert_eq!(order(&s, &sounds), ["Alpha", "bravo", "charlie"]);
        s.sort = SortMode::MostPlayed;
        assert_eq!(order(&s, &sounds), ["bravo", "charlie", "Alpha"]);
        s.sort = SortMode::RecentlyAdded;
        assert_eq!(order(&s, &sounds), ["bravo", "Alpha", "charlie"]);
        s.sort = SortMode::Manual;
        reorder(&mut sounds, 2, 0); // drag bravo onto charlie
        assert_eq!(order(&s, &sounds), ["bravo", "charlie", "Alpha"]);
        reorder(&mut sounds, 0, 9); // out of range: nothing happens
        assert_eq!(sounds[0].name, "bravo");

        // The order a sort shows can become the manual order (before a drag in that view).
        let by_name = sorted_order(&sounds, SortMode::Name);
        apply_order(&mut sounds, &by_name);
        assert_eq!(order(&s, &sounds), ["Alpha", "bravo", "charlie"]);
        apply_order(&mut sounds, &[0]); // not a full permutation: untouched
        assert_eq!(sounds.len(), 3);
    }

    #[test]
    fn repair_forgets_missing_folders_and_recents() {
        let mut s = BoardSettings {
            folders: vec![Folder {
                id: 1,
                name: "Kept".into(),
            }],
            recent: vec![7, 2, 99],
            tab: BoardTab::Folder(5),
            ..Default::default()
        };
        let mut sounds = vec![clip(2, "a"), clip(7, "b")];
        sounds[0].folder = Some(1);
        sounds[1].folder = Some(5); // a folder that does not exist
        s.repair(&mut sounds);
        assert_eq!(sounds[0].folder, Some(1));
        assert_eq!(sounds[1].folder, None);
        assert_eq!(s.tab, BoardTab::All);
        assert_eq!(s.recent, [7, 2]);
    }
}
