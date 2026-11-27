use std::collections::{HashMap, HashSet};

use super::{GlyphAtlasError, GlyphKey, bitmap::Bitmap};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Slot {
    pub page: usize,
    pub offset: [u32; 2],
    pub bitmap: Bitmap,
}

#[derive(Clone, Copy, Default)]
struct Shelf {
    x: u32,
    y: u32,
    height: u32,
}

impl Shelf {
    fn place(mut self, [width, height]: [u32; 2], side: u32) -> Option<([u32; 2], Self)> {
        if width > side - self.x {
            self.x = 0;
            self.y += self.height;
            self.height = 0;
        }
        if height > side - self.y {
            return None;
        }
        let offset = [self.x, self.y];
        self.x += width;
        self.height = self.height.max(height);
        Some((offset, self))
    }
}

#[derive(Default)]
struct Page {
    shelf: Shelf,
    keys: Vec<GlyphKey>,
    last_use: u64,
    pinned: bool,
}

pub(super) struct Reservation {
    pub slot: Slot,
    shelf: Shelf,
    pub recycle: bool,
}

pub(super) struct Cache {
    pages: Vec<Page>,
    entries: HashMap<GlyphKey, Slot>,
    blank: HashSet<GlyphKey>,
    frame: u64,
    pub side: u32,
    max_pages: usize,
}

impl Cache {
    pub fn new(side: u32, max_pages: usize) -> Self {
        Self {
            pages: Vec::new(),
            entries: HashMap::new(),
            blank: HashSet::new(),
            frame: 0,
            side,
            max_pages,
        }
    }

    pub fn clear(&mut self) {
        self.pages.clear();
        self.entries.clear();
        self.blank.clear();
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
        for page in &mut self.pages {
            page.pinned = false;
        }
    }

    pub fn get(&mut self, key: GlyphKey) -> Option<Slot> {
        let slot = *self.entries.get(&key)?;
        let page = &mut self.pages[slot.page];
        page.pinned = true;
        page.last_use = self.frame;
        Some(slot)
    }

    pub fn is_blank(&self, key: GlyphKey) -> bool {
        self.blank.contains(&key)
    }

    pub fn mark_blank(&mut self, key: GlyphKey) {
        // Blank glyphs have no pixel slots to bound their metadata.
        if self.blank.len() == 256 {
            self.blank.clear();
        }
        self.blank.insert(key);
    }

    pub fn reserve(&self, bitmap: Bitmap) -> Result<Reservation, GlyphAtlasError> {
        if let Some(reservation) = self.pages.iter().enumerate().find_map(|(page, state)| {
            state
                .shelf
                .place(bitmap.size, self.side)
                .map(|(offset, shelf)| Reservation {
                    slot: Slot {
                        page,
                        offset,
                        bitmap,
                    },
                    shelf,
                    recycle: false,
                })
        }) {
            return Ok(reservation);
        }
        let (page, recycle) = if self.pages.len() < self.max_pages {
            (self.pages.len(), false)
        } else {
            let page = self
                .pages
                .iter()
                .enumerate()
                .filter(|(_, page)| !page.pinned)
                .min_by_key(|(_, page)| page.last_use)
                .map(|(index, _)| index)
                .ok_or(GlyphAtlasError::FrameFull)?;
            (page, true)
        };
        let (offset, shelf) = Shelf::default()
            .place(bitmap.size, self.side)
            .expect("raster fits an empty page");
        Ok(Reservation {
            slot: Slot {
                page,
                offset,
                bitmap,
            },
            shelf,
            recycle,
        })
    }

    pub fn commit(&mut self, key: GlyphKey, reservation: Reservation) -> Slot {
        let slot = reservation.slot;
        if slot.page == self.pages.len() {
            self.pages.push(Page::default());
        }
        let page = &mut self.pages[slot.page];
        if reservation.recycle {
            for previous in page.keys.drain(..) {
                self.entries.remove(&previous);
            }
        }
        page.shelf = reservation.shelf;
        page.keys.push(key);
        page.pinned = true;
        page.last_use = self.frame;
        self.entries.insert(key, slot);
        slot
    }
}
