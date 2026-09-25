//! Campaign flow (`campaign.toml`) and the persistent army state carried between battles.

use crate::data::{Equipment, Id, ItemKind};
use crate::pack::Pack;
use crate::script::Compare;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn ne() -> Compare {
    Compare::Ne
}

/// One step of the campaign. Nodes are visited in order of their `next` links.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    /// Play a drama scene (global scene id), then go to `next`.
    Drama { id: Id, scene: String, next: Id },
    /// Preparation screen before a battle: shop, equipment, deployment, saving.
    Camp {
        id: Id,
        /// Heading shown on the camp screen, e.g. `탁현 — 출진 준비`.
        #[serde(default)]
        title: String,
        /// Items the shop sells here.
        #[serde(default)]
        shop: Vec<Id>,
        /// Battle whose deployment this camp prepares (enables the deploy screen).
        #[serde(default)]
        battle: Option<Id>,
        next: Id,
    },
    /// Fight a battle. Victory goes to `next`; defeat goes to `on_defeat` or game over.
    Battle {
        id: Id,
        battle: Id,
        next: Id,
        #[serde(default)]
        on_defeat: Option<Id>,
    },
    /// Jump on a campaign flag: `flag <cmp> value` ? then : else.
    Branch {
        id: Id,
        flag: String,
        #[serde(default = "ne")]
        cmp: Compare,
        #[serde(default)]
        value: i64,
        then: Id,
        #[serde(rename = "else")]
        otherwise: Id,
    },
    /// The end of the campaign (optionally after a final scene).
    Ending {
        id: Id,
        #[serde(default)]
        scene: Option<String>,
        /// Ending title shown on the credits screen.
        #[serde(default)]
        title: String,
    },
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::Drama { id, .. }
            | Node::Camp { id, .. }
            | Node::Battle { id, .. }
            | Node::Branch { id, .. }
            | Node::Ending { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignDef {
    pub title: String,
    /// First node of a new game.
    pub start: Id,
    /// Officers in the army at the start of a new game.
    pub starting_officers: Vec<Id>,
    #[serde(default)]
    pub starting_gold: i64,
    /// Starting inventory: item id -> count.
    #[serde(default)]
    pub starting_items: BTreeMap<Id, u32>,
    #[serde(rename = "node")]
    pub nodes: Vec<Node>,
}

impl CampaignDef {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id() == id)
    }
}

/// Persistent per-officer progress for officers in the player's army.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficerState {
    pub id: Id,
    pub class: Id,
    pub level: u32,
    pub exp: u32,
    #[serde(rename = "str")]
    pub strength: i32,
    pub int: i32,
    pub lead: i32,
    pub equip: Equipment,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampaignError {
    #[error("unknown officer `{0}`")]
    UnknownOfficer(Id),
    #[error("officer `{0}` is not in the army")]
    NotInArmy(Id),
    #[error("unknown item `{0}`")]
    UnknownItem(Id),
    #[error("item `{0}` is not in the inventory")]
    NotOwned(Id),
    #[error("not enough gold: need {need}, have {have}")]
    NotEnoughGold { need: i64, have: i64 },
    #[error("item `{item}` cannot be equipped by class family `{family}`")]
    CannotEquip { item: Id, family: Id },
    #[error("item `{0}` is not equipment")]
    NotEquipment(Id),
    #[error("item `{0}` cannot be sold")]
    CannotSell(Id),
    #[error("item `{item}` cannot be used on `{officer}`: {reason}")]
    CannotUse { item: Id, officer: Id, reason: String },
    #[error("unknown campaign node `{0}`")]
    UnknownNode(Id),
}

/// Everything that persists between battles; stored in save games.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignState {
    /// Current campaign node id.
    pub node: Id,
    /// Officers in the player's army, in roster order.
    pub roster: Vec<OfficerState>,
    /// Unequipped items: item id -> count.
    pub inventory: BTreeMap<Id, u32>,
    pub gold: i64,
    pub flags: BTreeMap<String, i64>,
    /// Officers chosen on the deploy screen for the upcoming battle.
    #[serde(default)]
    pub deployed: Vec<Id>,
    /// Battle ids won so far.
    #[serde(default)]
    pub battles_won: Vec<Id>,
    /// Total play time in seconds (maintained by the frontend).
    #[serde(default)]
    pub play_seconds: u64,
}

impl CampaignState {
    /// Fresh state for a new game: starting officers (at their `officers.toml` level/class/equipment),
    /// starting gold and items, positioned at `campaign.start`.
    pub fn new_game(pack: &Pack) -> CampaignState {
        todo!("W1b: CampaignState::new_game {}", pack.manifest.id)
    }

    pub fn officer(&self, id: &str) -> Option<&OfficerState> {
        self.roster.iter().find(|o| o.id == id)
    }

    pub fn officer_mut(&mut self, id: &str) -> Option<&mut OfficerState> {
        self.roster.iter_mut().find(|o| o.id == id)
    }

    pub fn flag(&self, name: &str) -> i64 {
        self.flags.get(name).copied().unwrap_or(0)
    }

    /// Add an officer to the army (no-op if already present).
    pub fn join(&mut self, pack: &Pack, officer: &str) -> Result<(), CampaignError> {
        todo!("W1b: join {officer} {}", pack.manifest.id)
    }

    /// Remove an officer from the army, returning their equipment to the inventory.
    pub fn leave(&mut self, officer: &str) -> Result<(), CampaignError> {
        todo!("W1b: leave {officer}")
    }

    pub fn add_item(&mut self, item: &str, count: u32) {
        *self.inventory.entry(item.to_string()).or_insert(0) += count;
    }

    pub fn remove_item(&mut self, item: &str) -> Result<(), CampaignError> {
        todo!("W1b: remove_item {item}")
    }

    /// Change gold, clamped to `0..=rules.gold_cap`.
    pub fn add_gold(&mut self, pack: &Pack, amount: i64) {
        todo!("W1b: add_gold {amount} {}", pack.manifest.id)
    }

    pub fn buy(&mut self, pack: &Pack, item: &str) -> Result<(), CampaignError> {
        todo!("W1b: buy {item} {}", pack.manifest.id)
    }

    /// Sell for half the price (items with price 0 cannot be sold).
    pub fn sell(&mut self, pack: &Pack, item: &str) -> Result<(), CampaignError> {
        todo!("W1b: sell {item} {}", pack.manifest.id)
    }

    /// Equip an inventory item on an officer; the previously equipped item of that slot
    /// goes back to the inventory. Checks `ItemDef::families` against the officer's class family.
    pub fn equip(&mut self, pack: &Pack, officer: &str, item: &str) -> Result<(), CampaignError> {
        todo!("W1b: equip {officer} {item} {}", pack.manifest.id)
    }

    pub fn unequip(&mut self, officer: &str, slot: ItemKind) -> Result<(), CampaignError> {
        todo!("W1b: unequip {officer} {slot:?}")
    }

    /// Use a camp consumable (class-up `Promote` / `ChangeClass` items) on an officer.
    /// Checks the promotion level and item, `OfficerDef::fixed_class`, then consumes the item.
    pub fn use_item(&mut self, pack: &Pack, officer: &str, item: &str) -> Result<(), CampaignError> {
        todo!("W1b: use_item {officer} {item} {}", pack.manifest.id)
    }

    /// Resolve the node after the current one. Branch nodes are evaluated immediately
    /// (following chains of branches); returns the new current node id.
    pub fn advance(&mut self, pack: &Pack) -> Result<Id, CampaignError> {
        todo!("W1b: advance {}", pack.manifest.id)
    }

    /// Apply a finished battle: copy level/exp/class/stat changes of deployed officers back,
    /// add won gold/items, record the victory and set any flags the battle set.
    pub fn apply_battle_result(&mut self, pack: &Pack, battle: &crate::battle::BattleState) {
        todo!("W1b: apply_battle_result {} {}", pack.manifest.id, battle.battle_id)
    }
}
