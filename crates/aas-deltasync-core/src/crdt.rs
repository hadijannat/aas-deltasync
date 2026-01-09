//! CRDT primitives for AAS-ΔSync.
//!
//! Provides Last-Writer-Wins registers and Observed-Remove Maps
//! adapted for AAS Submodel semantics.

use crate::hlc::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A Last-Writer-Wins register holding a value with a timestamp.
///
/// Merge always takes the value with the higher timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    /// The stored value
    pub value: T,
    /// Timestamp of the last write
    pub timestamp: Timestamp,
}

impl<T: Clone> LwwRegister<T> {
    /// Create a new register with an initial value.
    #[must_use]
    pub fn new(value: T, timestamp: Timestamp) -> Self {
        Self { value, timestamp }
    }

    /// Update the register value if the new timestamp is higher.
    ///
    /// Returns `true` if the value was updated.
    pub fn set(&mut self, value: T, timestamp: Timestamp) -> bool {
        if timestamp > self.timestamp {
            self.value = value;
            self.timestamp = timestamp;
            true
        } else {
            false
        }
    }

    /// Merge with another register, keeping the value with the higher timestamp.
    pub fn merge(&mut self, other: &Self) {
        if other.timestamp > self.timestamp {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
        }
    }

    /// Generate a delta representing the current state.
    #[must_use]
    pub fn to_delta(&self) -> RegisterDelta<T>
    where
        T: Clone,
    {
        RegisterDelta {
            value: self.value.clone(),
            timestamp: self.timestamp,
        }
    }
}

/// A delta for LWW register replication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterDelta<T> {
    /// The value to replicate
    pub value: T,
    /// Timestamp of the value
    pub timestamp: Timestamp,
}

/// An Observed-Remove Map keyed by path segments.
///
/// Supports add, update, and remove operations with causal consistency.
/// Removed entries are tracked by tombstones until compaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    /// Active entries
    entries: HashMap<K, MapEntry<V>>,
    /// Tombstones for removed entries (key -> removal timestamp)
    tombstones: HashMap<K, Timestamp>,
}

/// An entry in the OR-Map with per-entry metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapEntry<V> {
    /// The stored value (itself an LWW register)
    pub value: LwwRegister<V>,
    /// Entry creation timestamp (for add-wins semantics)
    pub created_at: Timestamp,
}

impl<K, V> Default for OrMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> OrMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    /// Create a new empty OR-Map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            tombstones: HashMap::new(),
        }
    }

    /// Get a value by key.
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|e| &e.value.value)
    }

    /// Get a mutable reference to an entry.
    #[must_use]
    pub fn get_entry(&self, key: &K) -> Option<&MapEntry<V>> {
        self.entries.get(key)
    }

    /// Check if a key exists.
    #[must_use]
    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Insert or update a value.
    ///
    /// Returns `true` if this was an insert (vs update).
    pub fn insert(&mut self, key: K, value: V, timestamp: Timestamp) -> bool {
        // Check if there's a tombstone that supersedes this insert
        if let Some(&tombstone_ts) = self.tombstones.get(&key) {
            if tombstone_ts >= timestamp {
                // Removal happened after this insert, ignore
                return false;
            }
            // Insert is newer, remove tombstone
            self.tombstones.remove(&key);
        }

        let is_new = !self.entries.contains_key(&key);

        self.entries
            .entry(key)
            .and_modify(|e| {
                e.value.set(value.clone(), timestamp);
            })
            .or_insert_with(|| MapEntry {
                value: LwwRegister::new(value, timestamp),
                created_at: timestamp,
            });

        is_new
    }

    /// Remove a key.
    ///
    /// Returns the removed value if it existed.
    pub fn remove(&mut self, key: &K, timestamp: Timestamp) -> Option<V> {
        // Record tombstone
        self.tombstones
            .entry(key.clone())
            .and_modify(|ts| {
                if timestamp > *ts {
                    *ts = timestamp;
                }
            })
            .or_insert(timestamp);

        // Remove entry if tombstone supersedes it
        if let Some(entry) = self.entries.get(key) {
            if timestamp > entry.value.timestamp {
                return self.entries.remove(key).map(|e| e.value.value);
            }
        }

        None
    }

    /// Merge with another OR-Map.
    pub fn merge(&mut self, other: &Self) {
        // Merge tombstones (keep latest)
        for (key, &other_ts) in &other.tombstones {
            self.tombstones
                .entry(key.clone())
                .and_modify(|ts| {
                    if other_ts > *ts {
                        *ts = other_ts;
                    }
                })
                .or_insert(other_ts);
        }

        // Merge entries
        for (key, other_entry) in &other.entries {
            // Check if our tombstone supersedes this entry
            if let Some(&tombstone_ts) = self.tombstones.get(key) {
                if tombstone_ts >= other_entry.value.timestamp {
                    continue;
                }
            }

            self.entries
                .entry(key.clone())
                .and_modify(|e| {
                    e.value.merge(&other_entry.value);
                    // Keep earliest created_at
                    if other_entry.created_at < e.created_at {
                        e.created_at = other_entry.created_at;
                    }
                })
                .or_insert_with(|| other_entry.clone());
        }

        // Remove entries that are superseded by tombstones
        self.entries.retain(|key, entry| {
            if let Some(&tombstone_ts) = self.tombstones.get(key) {
                entry.value.timestamp > tombstone_ts
            } else {
                true
            }
        });
    }

    /// Get an iterator over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, e)| (k, &e.value.value))
    }

    /// Get the number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Compact tombstones older than the given timestamp.
    ///
    /// This is safe only after all peers have synced past the timestamp.
    pub fn compact_tombstones(&mut self, before: Timestamp) {
        self.tombstones.retain(|_, &mut ts| ts >= before);
    }
}

/// A delta representing changes to an OR-Map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delta<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    /// Inserted or updated entries
    pub inserts: Vec<(K, V, Timestamp)>,
    /// Removed keys
    pub removes: Vec<(K, Timestamp)>,
}

impl<K, V> Default for Delta<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Delta<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    /// Create an empty delta.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inserts: Vec::new(),
            removes: Vec::new(),
        }
    }

    /// Record an insert/update.
    pub fn add_insert(&mut self, key: K, value: V, timestamp: Timestamp) {
        self.inserts.push((key, value, timestamp));
    }

    /// Record a removal.
    pub fn add_remove(&mut self, key: K, timestamp: Timestamp) {
        self.removes.push((key, timestamp));
    }

    /// Check if the delta is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inserts.is_empty() && self.removes.is_empty()
    }

    /// Apply this delta to an OR-Map.
    pub fn apply_to(&self, map: &mut OrMap<K, V>) {
        for (key, value, timestamp) in &self.inserts {
            map.insert(key.clone(), value.clone(), *timestamp);
        }
        for (key, timestamp) in &self.removes {
            map.remove(key, *timestamp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_timestamp(physical: u64, logical: u32, actor_num: u8) -> Timestamp {
        Timestamp {
            physical_ms: physical,
            logical,
            actor_id: Uuid::from_bytes([actor_num; 16]),
        }
    }

    #[test]
    fn lww_register_higher_timestamp_wins() {
        let t1 = make_timestamp(1000, 0, 1);
        let t2 = make_timestamp(2000, 0, 1);

        let mut reg = LwwRegister::new(10, t1);
        assert_eq!(reg.value, 10);

        reg.set(20, t2);
        assert_eq!(reg.value, 20);

        // Earlier timestamp should not update
        reg.set(5, t1);
        assert_eq!(reg.value, 20);
    }

    #[test]
    fn ormap_basic_operations() {
        let t1 = make_timestamp(1000, 0, 1);
        let t2 = make_timestamp(2000, 0, 1);

        let mut map: OrMap<String, i32> = OrMap::new();

        assert!(map.insert("a".to_string(), 1, t1));
        assert!(!map.insert("a".to_string(), 2, t2)); // Update, not insert

        assert_eq!(map.get(&"a".to_string()), Some(&2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn ormap_remove_supersedes() {
        let t1 = make_timestamp(1000, 0, 1);
        let t2 = make_timestamp(2000, 0, 1);
        let t3 = make_timestamp(3000, 0, 1);

        let mut map: OrMap<String, i32> = OrMap::new();

        map.insert("a".to_string(), 1, t1);
        map.remove(&"a".to_string(), t2);

        assert!(map.get(&"a".to_string()).is_none());

        // Insert with earlier timestamp should be ignored (tombstone wins)
        map.insert("a".to_string(), 2, t1);
        assert!(map.get(&"a".to_string()).is_none());

        // Insert with later timestamp should succeed
        map.insert("a".to_string(), 3, t3);
        assert_eq!(map.get(&"a".to_string()), Some(&3));
    }

    #[test]
    fn ormap_merge_convergence() {
        let t1 = make_timestamp(1000, 0, 1);
        let t2 = make_timestamp(2000, 0, 2);

        let mut map_a: OrMap<String, i32> = OrMap::new();
        let mut map_b: OrMap<String, i32> = OrMap::new();

        // Concurrent inserts
        map_a.insert("x".to_string(), 10, t1);
        map_b.insert("x".to_string(), 20, t2);

        // Merge in both directions
        let mut merged_a = map_a.clone();
        merged_a.merge(&map_b);

        let mut merged_b = map_b.clone();
        merged_b.merge(&map_a);

        // Should converge to same state (t2 wins)
        assert_eq!(merged_a.get(&"x".to_string()), Some(&20));
        assert_eq!(merged_b.get(&"x".to_string()), Some(&20));
    }

    #[test]
    fn delta_apply() {
        let t1 = make_timestamp(1000, 0, 1);
        let t2 = make_timestamp(2000, 0, 1);

        let mut delta: Delta<String, i32> = Delta::new();
        delta.add_insert("a".to_string(), 1, t1);
        delta.add_insert("b".to_string(), 2, t1);
        delta.add_remove("a".to_string(), t2);

        let mut map: OrMap<String, i32> = OrMap::new();
        delta.apply_to(&mut map);

        assert!(map.get(&"a".to_string()).is_none());
        assert_eq!(map.get(&"b".to_string()), Some(&2));
    }
}
