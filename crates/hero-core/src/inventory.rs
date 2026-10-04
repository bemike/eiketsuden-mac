//! Personal, eight-slot inventories. Transfers are atomic and retain duplicate items.
use crate::data::Id;
use serde::{Deserialize, Serialize};

pub const CAPACITY: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(from = "[String; CAPACITY]", into = "[String; CAPACITY]")]
pub struct Pocket([Option<Id>; CAPACITY]);

// TOML has no null array elements; empty strings encode empty physical slots in both
// TOML pack data and JSON saves, while runtime code uses Option explicitly.
impl From<[String; CAPACITY]> for Pocket {
    fn from(items: [String; CAPACITY]) -> Self {
        Self(items.map(|id| if id.is_empty() { None } else { Some(id) }))
    }
}
impl From<Pocket> for [String; CAPACITY] {
    fn from(pocket: Pocket) -> Self {
        pocket.0.map(Option::unwrap_or_default)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InventoryError {
    #[error("道具栏已满（最多八件）。")]
    Full,
    #[error("道具编号不能为空。")]
    EmptyId,
    #[error("道具位置无效。")]
    BadSlot,
    #[error("此位置没有道具。")]
    Empty,
    #[error("未持有此道具。")]
    NotOwned,
}

impl Pocket {
    pub fn from_items(items: impl IntoIterator<Item = Id>) -> Result<Self, InventoryError> {
        let mut pocket = Self::default();
        for item in items {
            pocket.insert(item)?;
        }
        Ok(pocket)
    }
    pub fn slots(&self) -> &[Option<Id>; CAPACITY] {
        &self.0
    }
    pub fn iter(&self) -> impl Iterator<Item = &Id> {
        self.0.iter().flatten()
    }
    pub fn count(&self, item: &str) -> u32 {
        self.iter().filter(|id| id.as_str() == item).count() as u32
    }
    pub fn insert(&mut self, item: Id) -> Result<usize, InventoryError> {
        if item.is_empty() {
            return Err(InventoryError::EmptyId);
        }
        let slot = self
            .0
            .iter()
            .position(Option::is_none)
            .ok_or(InventoryError::Full)?;
        self.0[slot] = Some(item);
        Ok(slot)
    }
    pub fn take(&mut self, slot: usize) -> Result<Id, InventoryError> {
        self.0
            .get_mut(slot)
            .ok_or(InventoryError::BadSlot)?
            .take()
            .ok_or(InventoryError::Empty)
    }
    pub fn consume(&mut self, item: &str) -> Result<(), InventoryError> {
        let slot = self
            .0
            .iter()
            .position(|v| v.as_deref() == Some(item))
            .ok_or(InventoryError::NotOwned)?;
        self.take(slot)?;
        Ok(())
    }
    /// An occupied target swaps; an empty target transfers. Check both slots before mutation.
    pub fn exchange(
        &mut self,
        source: usize,
        other: &mut Pocket,
        target: usize,
    ) -> Result<(), InventoryError> {
        if source >= CAPACITY || target >= CAPACITY {
            return Err(InventoryError::BadSlot);
        }
        if self.0[source].is_none() {
            return Err(InventoryError::Empty);
        }
        std::mem::swap(&mut self.0[source], &mut other.0[target]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn full() -> Pocket {
        Pocket::from_items((0..8).map(|n| format!("item{n}"))).unwrap()
    }
    #[test]
    fn duplicate_items_are_separate_slots_and_full_bags_can_exchange() {
        let mut a = Pocket::from_items(vec!["bean".into(); 8]).unwrap();
        let mut b = full();
        a.exchange(3, &mut b, 7).unwrap();
        assert_eq!(a.count("bean"), 7);
        assert_eq!(a.slots()[3].as_deref(), Some("item7"));
        assert_eq!(b.slots()[7].as_deref(), Some("bean"));
        assert_eq!(a.insert("wine".into()), Err(InventoryError::Full));
    }
    #[test]
    fn transfer_and_consume_change_only_the_owner_and_one_copy() {
        let mut a = Pocket::from_items(vec!["bean".into(); 2]).unwrap();
        let mut b = Pocket::default();
        a.exchange(0, &mut b, 4).unwrap();
        b.consume("bean").unwrap();
        assert_eq!(a.count("bean"), 1);
        assert_eq!(b.count("bean"), 0);
        assert_eq!(b.consume("bean"), Err(InventoryError::NotOwned));
    }
    #[test]
    fn rejected_exchange_is_atomic() {
        let mut a = full();
        let mut b = Pocket::default();
        let before = (a.clone(), b.clone());
        assert_eq!(a.exchange(0, &mut b, 8), Err(InventoryError::BadSlot));
        assert_eq!((a.clone(), b.clone()), before);
        assert_eq!(b.exchange(0, &mut a, 0), Err(InventoryError::Empty));
        assert_eq!((a, b), before);
    }
    #[test]
    fn save_roundtrip_preserves_empty_slots_duplicates_and_capacity() {
        let mut p = Pocket::from_items(vec!["bean".into(); 8]).unwrap();
        p.take(2).unwrap();
        assert_eq!(
            serde_json::from_str::<Pocket>(&serde_json::to_string(&p).unwrap()).unwrap(),
            p
        );
        assert!(serde_json::from_str::<Pocket>("[]").is_err());
        assert_eq!(
            Pocket::from_items(vec!["bean".into(); 9]),
            Err(InventoryError::Full)
        );
    }
}
