//! Per-search state indexed by interned code (DS-225): what the searches
//! use instead of hashing a connection's UID and TIPLOCs into maps.
//!
//! [`schedule_query::StopCode`] and [`schedule_query::TripCode`] are dense
//! from 0, so a vector of [`schedule_query::StopCode::count`] entries
//! indexes every stop. A code interned after the vector was sized (another
//! request building a graph concurrently) is past its end: the vectors
//! grow on write and read as empty past the end.

use std::collections::{HashMap, HashSet};

use schedule_query::{
    FixedLink, InterchangeData, ModalChangeBuffer, TripCode, links_in_lookup_order,
};

/// `FxHash`, as rustc uses: far cheaper than the default `SipHash` for the
/// short TIPLOC and UID keys of a scan's own per-query maps, which are
/// looked up for nearly every connection. Not resistant to collision
/// attacks, which is fine: the keys are timetable identifiers, not request
/// input.
#[derive(Default, Clone, Copy)]
pub(crate) struct FxHasher(u64);

impl FxHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.add(u64::from_le_bytes(*word));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, byte: u8) {
        self.add(u64::from(byte));
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type FxMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FxHasher>>;
pub(crate) type FxSet<K> = HashSet<K, std::hash::BuildHasherDefault<FxHasher>>;

/// A `u32` per code, [`DenseMap::NONE`] where nothing is recorded.
#[derive(Debug, Clone, Default)]
pub(crate) struct DenseMap(Vec<u32>);

impl DenseMap {
    pub(crate) const NONE: u32 = u32::MAX;

    pub(crate) fn with_len(len: usize) -> Self {
        Self(vec![Self::NONE; len])
    }

    pub(crate) fn get(&self, index: usize) -> Option<u32> {
        self.0
            .get(index)
            .copied()
            .filter(|&value| value != Self::NONE)
    }

    pub(crate) fn set(&mut self, index: usize, value: u32) {
        if index >= self.0.len() {
            self.0.resize(index + 1, Self::NONE);
        }
        self.0[index] = value;
    }

    pub(crate) fn clear(&mut self, index: usize) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = Self::NONE;
        }
    }
}

/// One fixed link from a stop, and the TIPLOCs at its other end: a
/// search's cache of the interchange data's links, looked up once per stop
/// instead of on every expansion (DS-225). The far TIPLOCs are resolved to
/// the search's stops only when the link is walked.
pub(crate) struct StopLink<'a> {
    pub(crate) link: &'a FixedLink,
    pub(crate) to: &'a [String],
}

impl<'a> StopLink<'a> {
    /// `crs`'s links in lookup order (`schedule_query::links_in_lookup_order`).
    pub(crate) fn of(interchange: &'a InterchangeData, crs: &str) -> Vec<Self> {
        links_in_lookup_order(interchange, crs)
            .into_iter()
            .map(|link| StopLink {
                link,
                to: interchange
                    .crs_to_tiplocs
                    .get(&link.to_crs)
                    .map_or(&[], Vec::as_slice),
            })
            .collect()
    }
}

/// [`ModalChangeBuffer`] by [`TripCode`]: one flag read instead of hashing
/// the UID into its `HashSet<String>` at every boarding and alighting.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModalBuffer {
    minutes: u32,
    road_or_water: Vec<bool>,
}

impl ModalBuffer {
    pub(crate) fn new(buffer: &ModalChangeBuffer) -> Self {
        if buffer.minutes == 0 || buffer.road_or_water_uids.is_empty() {
            return Self::default();
        }
        let mut road_or_water = vec![false; TripCode::count()];
        // A UID never interned is on no connection.
        for uid in &buffer.road_or_water_uids {
            if let Some(code) = TripCode::find(uid) {
                if code.index() >= road_or_water.len() {
                    road_or_water.resize(code.index() + 1, false);
                }
                road_or_water[code.index()] = true;
            }
        }
        Self {
            minutes: buffer.minutes,
            road_or_water,
        }
    }

    /// `ModalChangeBuffer::extra_for`, by code.
    pub(crate) fn extra_for(&self, uid: TripCode) -> u32 {
        if self
            .road_or_water
            .get(uid.index())
            .copied()
            .unwrap_or(false)
        {
            self.minutes
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dense_map_grows_on_write_and_reads_empty_past_its_end() {
        let mut map = DenseMap::with_len(2);
        assert_eq!(map.get(1), None);
        assert_eq!(map.get(10), None);
        map.set(10, 7);
        assert_eq!(map.get(10), Some(7));
        map.clear(10);
        assert_eq!(map.get(10), None);
    }

    #[test]
    fn the_modal_buffer_matches_the_uid_set() {
        let buffer = ModalChangeBuffer {
            road_or_water_uids: ["ZZDENSEBUS".to_string()].into(),
            minutes: 5,
        };
        let bus = TripCode::new("ZZDENSEBUS");
        let train = TripCode::new("ZZDENSETRAIN");
        let dense = ModalBuffer::new(&buffer);
        assert_eq!(dense.extra_for(bus), buffer.extra_for("ZZDENSEBUS"));
        assert_eq!(dense.extra_for(train), 0);
        assert_eq!(ModalBuffer::default().extra_for(bus), 0);
    }
}
