//! Generational handles and a slot arena. Stale handles resolve to `None`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

pub struct Handle<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _marker: PhantomData,
        }
    }

    pub fn index(&self) -> u32 {
        self.index
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<T> Eq for Handle<T> {}
impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

pub struct Store<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }
}

impl<T> fmt::Debug for Store<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store").field("len", &self.len).finish()
    }
}

impl<T> Store<T> {
    pub fn insert(&mut self, value: T) -> Handle<T> {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.value = Some(value);
            Handle::new(index, slot.generation)
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                value: Some(value),
            });
            Handle::new(index, 0)
        }
    }

    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        let slot = self.slots.get(handle.index as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_ref()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_mut()
        } else {
            None
        }
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }
        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(handle.index);
        self.len -= 1;
        Some(value)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value
                .as_ref()
                .map(|value| (Handle::new(index as u32, slot.generation), value))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_handles_do_not_resolve_after_slot_reuse() {
        let mut store = Store::default();
        let first = store.insert("a");
        assert_eq!(store.remove(first), Some("a"));
        assert!(store.get(first).is_none());
        let second = store.insert("b");
        assert_eq!(second.index(), first.index());
        assert!(store.get(first).is_none());
        assert_eq!(store.get(second), Some(&"b"));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn iter_skips_removed_entries() {
        let mut store = Store::default();
        let a = store.insert(1);
        store.insert(2);
        store.remove(a);
        let values: Vec<i32> = store.iter().map(|(_, v)| *v).collect();
        assert_eq!(values, vec![2]);
    }
}
