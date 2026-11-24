#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) struct SlotKey {
    index: usize,
    generation: u32,
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

pub(super) struct Slots<T> {
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Slots<T> {
    pub(super) fn insert(&mut self, value: T) -> SlotKey {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index];
            slot.value = Some(value);
            return SlotKey {
                index,
                generation: slot.generation,
            };
        }
        let index = self.slots.len();
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        SlotKey {
            index,
            generation: 0,
        }
    }

    pub(super) fn get(&self, key: SlotKey) -> Option<&T> {
        let slot = self.slots.get(key.index)?;
        (slot.generation == key.generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub(super) fn get_mut(&mut self, key: SlotKey) -> Option<&mut T> {
        let slot = self.slots.get_mut(key.index)?;
        (slot.generation == key.generation)
            .then_some(slot.value.as_mut())
            .flatten()
    }

    pub(super) fn remove(&mut self, key: SlotKey) -> Option<T> {
        let slot = self.slots.get_mut(key.index)?;
        if slot.generation != key.generation {
            return None;
        }
        let value = slot.value.take()?;
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            self.free.push(key.index);
        }
        Some(value)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (SlotKey, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value.as_ref().map(|value| {
                (
                    SlotKey {
                        index,
                        generation: slot.generation,
                    },
                    value,
                )
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Slots;

    #[test]
    fn reuses_released_slots_with_a_new_generation() {
        let mut slots = Slots::default();
        let first = slots.insert("first");
        assert_eq!(slots.remove(first), Some("first"));
        assert_eq!(slots.remove(first), None);
        let second = slots.insert("second");
        assert_eq!(first.index, second.index);
        assert_eq!(slots.get(first), None);
        assert_eq!(slots.get(second), Some(&"second"));
    }

    #[test]
    fn retires_slots_when_the_generation_cannot_advance() {
        let mut slots = Slots::default();
        let mut first = slots.insert(1);
        first.generation = u32::MAX;
        slots.slots[first.index].generation = first.generation;
        assert_eq!(slots.remove(first), Some(1));
        let second = slots.insert(2);
        assert_ne!(first.index, second.index);
        assert_eq!(slots.get(first), None);
    }
}
