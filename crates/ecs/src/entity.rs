//! Entity handles and the allocator that hands them out.
//!
//! Identity is decision 1 of ADR 0010: an index says where an entity lives, a
//! generation says which of the entities that have lived there it is. Only the
//! two together are a handle, which is what makes a handle kept across a
//! despawn detectable instead of silently addressing its successor.

use std::fmt;
use std::num::NonZeroU32;

/// The generation every slot starts at.
///
/// Generations are non-zero so that the zero bit pattern is free for the niche
/// that keeps `Option<Entity>` the same size as `Entity`.
const FIRST_GENERATION: NonZeroU32 = NonZeroU32::MIN;

/// An opaque handle to an entity.
///
/// Eight bytes: a 32-bit index and a 32-bit generation. `Option<Entity>` is
/// eight bytes as well, because the generation is non-zero and its zero bit
/// pattern is the niche - relations and hierarchies hold an optional entity in
/// most of their structures, so the difference is not academic.
///
/// A handle is only meaningful in the [`World`](crate::World) that produced it.
/// Entities are never serialized; authoring data refers to them through the
/// `AuthoringId` mapping of ADR 0004 instead.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entity {
    index: u32,
    generation: NonZeroU32,
}

impl Entity {
    /// Where the entity lives. Unique among live entities, reused after a
    /// despawn.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Which of the entities that have occupied this index it is. Starts at
    /// one and rises by one per despawn.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation.get()
    }
}

// Index and generation are one identity, so they are printed as one token
// rather than as a struct with two fields.
impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}v{})", self.index, self.generation)
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

/// One entry of the entity table.
///
/// A free slot carries the index of the next free one, which is what threads
/// the free list through the table instead of through a second `Vec`.
#[derive(Debug, Clone, Copy)]
enum Slot {
    Alive {
        generation: NonZeroU32,
    },
    Free {
        generation: NonZeroU32,
        next: Option<u32>,
    },
    /// Its generation cannot be raised again, so it is out of circulation for
    /// the life of the world.
    Retired,
}

/// The entity table and the free list over it.
#[derive(Debug, Default)]
pub(crate) struct Entities {
    slots: Vec<Slot>,
    free_head: Option<u32>,
    alive: u32,
}

impl Entities {
    /// Hands out a handle, reusing the most recently freed index if there is
    /// one.
    ///
    /// Reuse is last-in-first-out on purpose. The defect that outlives an
    /// entity is a side table indexed by entity index and never cleared on
    /// despawn; immediate reuse makes it corrupt data on the next spawn, where
    /// a test finds it, rather than a hundred thousand spawns later, where a
    /// user does.
    ///
    /// # Panics
    ///
    /// If every one of the 2^32 indices has been handed out and none is free.
    pub(crate) fn alloc(&mut self) -> Entity {
        let entity = match self.free_head {
            Some(index) => {
                let (generation, next) = match self.slots[index as usize] {
                    Slot::Free { generation, next } => (generation, next),
                    // The list is threaded through the slots themselves, so a
                    // slot reachable from the head that is not free means the
                    // two disagree - a bug in here, not in the caller.
                    other => unreachable!("free list reached a slot that is not free: {other:?}"),
                };
                self.free_head = next;
                self.slots[index as usize] = Slot::Alive { generation };
                Entity { index, generation }
            }
            None => {
                let index = u32::try_from(self.slots.len())
                    .expect("entity index space exhausted: 2^32 indices allocated or retired");
                self.slots.push(Slot::Alive {
                    generation: FIRST_GENERATION,
                });
                Entity {
                    index,
                    generation: FIRST_GENERATION,
                }
            }
        };

        self.alive += 1;
        entity
    }

    /// Frees the index behind a handle and returns whether the handle named a
    /// live entity.
    ///
    /// A handle that lost the race - the entity was despawned and its index
    /// handed out again - is rejected here rather than freeing its successor.
    pub(crate) fn free(&mut self, entity: Entity) -> bool {
        let next_free = self.free_head;
        let Some(slot) = self.slots.get_mut(entity.index as usize) else {
            return false;
        };
        let Slot::Alive { generation } = *slot else {
            return false;
        };
        if generation != entity.generation {
            return false;
        }

        match generation.checked_add(1) {
            Some(generation) => {
                *slot = Slot::Free {
                    generation,
                    next: next_free,
                };
                self.free_head = Some(entity.index);
            }
            // The generation is the whole of what tells a live handle from a
            // stale one. A slot that cannot be raised again would hand out a
            // handle indistinguishable from one four billion despawns old, so
            // it is retired instead: one index in 2^32 is the cheaper loss.
            None => *slot = Slot::Retired,
        }

        self.alive -= 1;
        true
    }

    /// Whether the handle names a live entity.
    pub(crate) fn contains(&self, entity: Entity) -> bool {
        matches!(
            self.slots.get(entity.index as usize),
            Some(Slot::Alive { generation }) if *generation == entity.generation
        )
    }

    /// How many entities are alive.
    pub(crate) fn len(&self) -> u32 {
        self.alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives a slot to the last generation it can hold, so that retirement is
    /// reachable without four billion despawns. Reaching into the table is why
    /// this lives next to it rather than in `tests/`.
    fn exhaust_generation(entities: &mut Entities, index: u32) {
        match &mut entities.slots[index as usize] {
            Slot::Alive { generation } | Slot::Free { generation, .. } => {
                *generation = NonZeroU32::MAX;
            }
            Slot::Retired => panic!("slot {index} is already retired"),
        }
    }

    #[test]
    fn a_handle_is_eight_bytes_and_so_is_an_optional_one() {
        assert_eq!(size_of::<Entity>(), 8);
        assert_eq!(size_of::<Option<Entity>>(), 8);
    }

    #[test]
    fn fresh_indices_are_handed_out_in_order() {
        let mut entities = Entities::default();

        let handles: Vec<Entity> = (0..3).map(|_| entities.alloc()).collect();

        assert_eq!(
            handles
                .iter()
                .map(|entity| (entity.index(), entity.generation()))
                .collect::<Vec<_>>(),
            vec![(0, 1), (1, 1), (2, 1)]
        );
    }

    #[test]
    fn a_freed_index_is_reused_by_the_next_spawn() {
        let mut entities = Entities::default();
        let first = entities.alloc();

        assert!(entities.free(first));
        let second = entities.alloc();

        assert_eq!(second.index(), first.index());
        assert_eq!(second.generation(), first.generation() + 1);
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn reuse_order_is_last_freed_first() {
        let mut entities = Entities::default();
        let a = entities.alloc();
        let b = entities.alloc();
        let c = entities.alloc();

        entities.free(a);
        entities.free(b);
        entities.free(c);

        assert_eq!(
            [
                entities.alloc().index(),
                entities.alloc().index(),
                entities.alloc().index()
            ],
            [c.index(), b.index(), a.index()]
        );
    }

    #[test]
    fn a_stale_handle_resolves_to_nothing_and_frees_nothing() {
        let mut entities = Entities::default();
        let stale = entities.alloc();
        entities.free(stale);
        let current = entities.alloc();

        assert!(!entities.contains(stale));
        assert!(!entities.free(stale));
        assert!(entities.contains(current));
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn a_handle_from_another_table_resolves_to_nothing() {
        let mut entities = Entities::default();
        let mut other = Entities::default();
        let foreign = other.alloc();

        assert!(!entities.contains(foreign));
        assert!(!entities.free(foreign));
    }

    #[test]
    fn a_slot_at_the_last_generation_is_retired_rather_than_reused() {
        let mut entities = Entities::default();
        let doomed = entities.alloc();
        let kept = entities.alloc();
        exhaust_generation(&mut entities, doomed.index());

        // The old handle is stale now: the generation moved under it.
        assert!(!entities.free(doomed));
        let doomed = Entity {
            index: doomed.index(),
            generation: NonZeroU32::MAX,
        };
        assert!(entities.free(doomed));

        let next = entities.alloc();
        assert_ne!(next.index(), doomed.index());
        assert_ne!(next.index(), kept.index());
        assert!(!entities.contains(doomed));
    }

    #[test]
    fn retiring_a_slot_does_not_disturb_the_free_list() {
        let mut entities = Entities::default();
        let retired = entities.alloc();
        let recycled = entities.alloc();

        entities.free(recycled);
        exhaust_generation(&mut entities, retired.index());
        let retired = Entity {
            index: retired.index(),
            generation: NonZeroU32::MAX,
        };
        entities.free(retired);

        assert_eq!(entities.alloc().index(), recycled.index());
        assert_eq!(entities.alloc().index(), 2);
    }
}
