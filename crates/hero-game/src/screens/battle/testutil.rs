//! Test fixtures: the base pack and the prologue battle `p1_sishui`.

use hero_core::battle::BattleState;
use hero_core::campaign::CampaignState;
use hero_core::pack::{DirSource, Pack};
use std::rc::Rc;

pub fn base_pack() -> Rc<Pack> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base");
    Rc::new(Pack::load(&DirSource { root }).expect("the base pack loads"))
}

/// The base pack and the first prologue battle, before `begin`.
pub fn sishui() -> (Rc<Pack>, BattleState) {
    let pack = base_pack();
    let campaign = CampaignState::new_game(&pack);
    let state = BattleState::new(&pack, "p1_sishui", &campaign, 7).expect("battle builds");
    (pack, state)
}
