//! Read-only, battle-specific notes derived from the same data the game executes.
//! Enemy equipment is deliberately excluded: only actual drops and rewards are obtainable.

use crate::battledef::{BattleDef, EventAction, Side, Trigger};
use crate::campaign::{CampaignState, Node};
use crate::data::{Effect, ItemKind};
use crate::pack::Pack;
use crate::script::{Cmd, DuelAct, DuelSide, Scene, SetOp};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideSection {
    pub title: String,
    pub entries: Vec<String>,
}

#[derive(Default)]
struct Notes {
    contacts: Vec<String>,
    treasures: Vec<String>,
    supplies: Vec<String>,
    classes: Vec<String>,
    post: BTreeMap<(String, usize), (PostReward, Path)>,
}

enum PostReward {
    Item(String),
    Gold(i64),
    Join(String),
}

fn add(entries: &mut Vec<String>, text: String) {
    if !entries.contains(&text) {
        entries.push(text);
    }
}

fn officer(pack: &Pack, id: &str) -> String {
    pack.officer(id)
        .map_or_else(|| id.to_string(), |o| o.name.clone())
}

fn target(pack: &Pack, battle: &BattleDef, id: &str) -> String {
    if let Some(u) = battle.units.iter().find(|u| u.tag.as_deref() == Some(id)) {
        if let Some(o) = &u.officer {
            return officer(pack, o);
        }
        if let Some(name) = &u.name {
            return name.clone();
        }
    }
    officer(pack, id)
}

fn position(p: crate::geom::Pos) -> String {
    format!("列{}、行{}", p.x + 1, p.y + 1)
}

fn source(pack: &Pack, battle: &BattleDef, trigger: &Trigger) -> String {
    match trigger {
        Trigger::Adjacent { a, b } => format!(
            "{}与{}上下左右相邻",
            a.as_deref()
                .map_or_else(|| "任意我方武将".into(), |a| target(pack, battle, a)),
            target(pack, battle, b)
        ),
        Trigger::UnitDefeated { target: id } => format!("击退{}", target(pack, battle, id)),
        Trigger::TurnStart { turn, .. } => format!("第{turn}回合剧情"),
        Trigger::Reach {
            who,
            pos,
            radius,
            to,
        } => {
            let who = who
                .as_deref()
                .map_or_else(|| "我方武将".into(), |id| target(pack, battle, id));
            if let Some(to) = to {
                format!("{who}到达{}至{}区域", position(*pos), position(*to))
            } else if *radius > 0 {
                format!("{who}接近{}（距离{radius}格内）", position(*pos))
            } else {
                format!("{who}到达{}", position(*pos))
            }
        }
        Trigger::HpBelow { target: id, pct } => {
            format!("{}兵力低于{pct}%", target(pack, battle, id))
        }
    }
}

fn is_conversion(pack: &Pack, id: &str) -> bool {
    pack.item(id).is_some_and(|i| {
        i.effects
            .iter()
            .any(|e| matches!(e, Effect::Promote | Effect::ChangeClass { .. }))
    })
}

fn conversion(pack: &Pack, id: &str) -> String {
    let Some(item) = pack.item(id) else {
        return id.into();
    };
    let effects: Vec<_> = item
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::ChangeClass { to } => Some(format!(
                "转为{}",
                pack.class(to).map_or(to.as_str(), |c| c.name.as_str())
            )),
            Effect::Promote => Some("晋升兵种，须达到相应等级".to_string()),
            _ => None,
        })
        .collect();
    format!("{}：{}", item.name, effects.join("；"))
}

impl Notes {
    fn record_post(&mut self, scene: &Scene, pc: usize, reward: PostReward, path: &Path) {
        let key = (scene.id.clone(), pc);
        let replace = self.post.get(&key).is_none_or(|(_, old)| {
            (
                path.choices.len(),
                path.conditional,
                reward_source(path).len(),
            ) < (old.choices.len(), old.conditional, reward_source(old).len())
        });
        if replace {
            self.post.insert(key, (reward, path.clone()));
        }
    }

    fn flush_post(&mut self, pack: &Pack) {
        for (_, (reward, path)) in std::mem::take(&mut self.post) {
            let at = reward_source(&path);
            match reward {
                PostReward::Item(id) => self.item(pack, &id, &at),
                PostReward::Gold(n) => add(&mut self.supplies, format!("军资金 {n} — {at}")),
                PostReward::Join(id) => add(
                    &mut self.contacts,
                    format!("{}加入 — {at}", officer(pack, &id)),
                ),
            }
        }
    }

    fn item(&mut self, pack: &Pack, id: &str, location: &str) {
        let Some(item) = pack.item(id) else { return };
        let text = format!("{} — {location}", item.name);
        if is_conversion(pack, id) {
            add(
                &mut self.classes,
                format!("{}；{location}", conversion(pack, id)),
            );
        } else if item.kind == ItemKind::Consumable {
            add(&mut self.supplies, text);
        } else {
            add(&mut self.treasures, text);
        }
    }
}

/// Summary for this exact battle, with story branches bounded by the next battle/camp.
/// This function never executes drama actions or changes a save.
pub fn battle_guide(
    pack: &Pack,
    battle_id: &str,
    campaign: &CampaignState,
    shop: &[String],
) -> Vec<GuideSection> {
    let Some(battle) = pack.battles.get(battle_id) else {
        return Vec::new();
    };
    let mut notes = Notes::default();
    for treasure in &battle.treasures {
        let at = format!("宝物格（{}），停留领取", position(treasure.pos));
        if let Some(item) = &treasure.item {
            notes.item(pack, item, &at);
        }
        if treasure.gold > 0 {
            add(
                &mut notes.supplies,
                format!("军资金 {} — {at}", treasure.gold),
            );
        }
    }
    for unit in &battle.units {
        if let Some(item) = &unit.drop {
            let who = unit.officer.as_deref().map_or_else(
                || unit.name.clone().unwrap_or_else(|| "敌军".into()),
                |id| officer(pack, id),
            );
            notes.item(pack, item, &format!("击退{who}后获得"));
        }
    }
    for event in &battle.events {
        let mut at = source(pack, battle, &event.trigger);
        if !event.when.is_empty() || !event.unless.is_empty() || event.stage.is_some_and(|s| s != 0)
        {
            at.push_str("（需前置剧情）");
        }
        let mut details = Vec::new();
        let mut duel = false;
        for action in &event.actions {
            match action {
                EventAction::Drama { scene } => {
                    if let Some(scene) = pack.scenes.get(scene) {
                        let mut fighters = None;
                        for cmd in &scene.cmds {
                            match cmd {
                                Cmd::Duel { left, right, .. } => {
                                    duel = true;
                                    fighters = Some((left, right));
                                }
                                Cmd::DuelAct {
                                    side,
                                    act: DuelAct::Fall,
                                } => {
                                    if let Some((left, right)) = fighters {
                                        let id = if *side == DuelSide::Left { left } else { right };
                                        if !battle.units.iter().any(|u| {
                                            u.officer.as_ref() == Some(id) && u.side == Side::Enemy
                                        }) {
                                            add(
                                                &mut details,
                                                format!("注意：{}败退", officer(pack, id)),
                                            );
                                        }
                                    }
                                }
                                Cmd::Join(id) => {
                                    add(&mut details, format!("{}加入", officer(pack, id)))
                                }
                                Cmd::Item(id) => notes.item(pack, id, &at),
                                Cmd::Gold(n) if *n > 0 => {
                                    add(&mut notes.supplies, format!("军资金 {n} — {at}"))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                EventAction::LevelUp { target: id, amount } => add(
                    &mut details,
                    format!("{}升{amount}级", target(pack, battle, id)),
                ),
                EventAction::Retreat { target: id } => {
                    add(&mut details, format!("{}撤退", target(pack, battle, id)))
                }
                EventAction::GiveItem { item } => notes.item(pack, item, &at),
                EventAction::GiveGold { amount } if *amount > 0 => {
                    add(&mut notes.supplies, format!("军资金 {amount} — {at}"))
                }
                EventAction::Victory => add(&mut details, "战斗胜利".into()),
                EventAction::Defeat => add(&mut details, "注意：触发战败".into()),
                _ => {}
            }
        }
        if matches!(event.trigger, Trigger::Adjacent { .. })
            || duel
            || details.iter().any(|d| d.ends_with("加入"))
        {
            let kind = if duel { "单挑" } else { "会面" };
            let detail = if details.is_empty() {
                "触发剧情".into()
            } else {
                details.join("；")
            };
            let entry = format!("{kind}：{at} — {detail}");
            let key = entry.replace("（需前置剧情）", "");
            if let Some(old) = notes
                .contacts
                .iter_mut()
                .find(|old| old.replace("（需前置剧情）", "") == key)
            {
                if entry.len() < old.len() {
                    *old = entry;
                }
            } else {
                notes.contacts.push(entry);
            }
        }
    }
    if battle.reward_gold > 0 {
        add(
            &mut notes.supplies,
            format!("过关军资金 {}", battle.reward_gold),
        );
    }
    after_battle(pack, battle, campaign, &mut notes);
    for (id, item) in &pack.items {
        if !is_conversion(pack, id) {
            continue;
        }
        let held = campaign.inventory.get(id).copied().unwrap_or(0) > 0
            || campaign
                .roster
                .iter()
                .any(|o| o.equip.carried.as_ref().is_some_and(|p| p.count(id) > 0));
        let mut availability = Vec::new();
        if shop.contains(id) {
            availability.push("本处商店可购买");
        }
        if held {
            availability.push("队伍已持有");
        }
        if !availability.is_empty() {
            add(
                &mut notes.classes,
                format!(
                    "{}；{}",
                    conversion(pack, &item.id),
                    availability.join("，")
                ),
            );
        }
    }
    [
        ("单挑与会面", notes.contacts, "本关没有已记录的单挑或会面。"),
        ("宝物与兵书", notes.treasures, "本关没有已记录的宝物奖励。"),
        ("物品与军资金", notes.supplies, "本关没有已记录的物品奖励。"),
        (
            "兵种转换",
            notes.classes,
            "本关没有转换道具奖励，队伍也未持有此类道具。",
        ),
    ]
    .into_iter()
    .map(|(title, mut entries, empty)| {
        if entries.is_empty() {
            entries.push(empty.into());
        }
        GuideSection {
            title: title.into(),
            entries,
        }
    })
    .collect()
}

// A tiny abstract interpreter follows choices and flag writes without applying gameplay actions.
// None means a flag can change during the coming battle; absent flags have the game's value 0.
type Flags = BTreeMap<String, Option<i64>>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Path {
    flags: Flags,
    choices: Vec<String>,
    conditional: bool,
}

fn test_flag(flags: &Flags, flag: &str, cmp: crate::script::Compare, value: i64) -> Option<bool> {
    flags
        .get(flag)
        .copied()
        .unwrap_or(Some(0))
        .map(|n| cmp.eval(n, value))
}

fn reward_source(path: &Path) -> String {
    if !path.choices.is_empty() {
        format!("战后剧情，选择「{}」", path.choices.join("」→「"))
    } else if path.conditional {
        "战后剧情（需满足剧情条件）".into()
    } else {
        "战后剧情".into()
    }
}

fn scene_paths(
    _pack: &Pack,
    scene: &Scene,
    path: Path,
    campaign: &CampaignState,
    notes: &mut Notes,
) -> Vec<Path> {
    let mut queue = VecDeque::from([(0, path)]);
    let mut visited = BTreeSet::new();
    let mut ends = Vec::new();
    while let Some((pc, mut path)) = queue.pop_front() {
        if !visited.insert((pc, path.flags.clone(), path.conditional)) {
            continue;
        }
        // Malformed mod scripts cannot freeze the camp screen.
        if visited.len() > 4096 {
            break;
        }
        let Some(cmd) = scene.cmds.get(pc) else {
            ends.push(path);
            continue;
        };
        match cmd {
            Cmd::End => {
                ends.push(path);
                continue;
            }
            Cmd::Goto(label) => {
                if let Some(to) = scene.label_index(label) {
                    queue.push_back((to, path));
                }
                continue;
            }
            Cmd::If { cond, label } => {
                let result = test_flag(&path.flags, &cond.flag, cond.cmp, cond.value);
                if result != Some(false) {
                    if let Some(to) = scene.label_index(label) {
                        let mut branch = path.clone();
                        branch.conditional |= result.is_none();
                        queue.push_back((to, branch));
                    }
                }
                if result != Some(true) {
                    path.conditional |= result.is_none();
                    queue.push_back((pc + 1, path));
                }
                continue;
            }
            Cmd::Choice(options) => {
                for option in options {
                    if let Some(to) = scene.label_index(&option.label) {
                        let mut branch = path.clone();
                        // Limit labels as well as execution: a broken choice loop remains bounded.
                        if branch.choices.len() < 8 {
                            branch.choices.push(option.text.clone());
                        }
                        branch.conditional = true;
                        queue.push_back((to, branch));
                    }
                }
                continue;
            }
            Cmd::Set { flag, op, value } => {
                let old = path.flags.get(flag).copied().unwrap_or(Some(0));
                let new = match op {
                    SetOp::Assign => Some(*value),
                    SetOp::Add => old.map(|n| n.saturating_add(*value)),
                    SetOp::Sub => old.map(|n| n.saturating_sub(*value)),
                };
                path.flags.insert(flag.clone(), new);
            }
            Cmd::Item(id) => notes.record_post(scene, pc, PostReward::Item(id.clone()), &path),
            Cmd::Gold(n) if *n > 0 => notes.record_post(scene, pc, PostReward::Gold(*n), &path),
            Cmd::Join(id) if !campaign.roster.iter().any(|o| o.id == *id && !o.away) => {
                notes.record_post(scene, pc, PostReward::Join(id.clone()), &path)
            }
            _ => {}
        }
        queue.push_back((pc + 1, path));
    }
    ends
}

fn after_battle(pack: &Pack, battle: &BattleDef, campaign: &CampaignState, notes: &mut Notes) {
    let mut flags: Flags = campaign
        .flags
        .iter()
        .map(|(k, v)| (k.clone(), Some(*v)))
        .collect();
    for event in &battle.events {
        for action in &event.actions {
            match action {
                EventAction::SetFlag { flag, .. } => {
                    flags.insert(flag.clone(), None);
                }
                EventAction::Drama { scene } => {
                    if let Some(scene) = pack.scenes.get(scene) {
                        for cmd in &scene.cmds {
                            if let Cmd::Set { flag, .. } = cmd {
                                flags.insert(flag.clone(), None);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let start = Path {
        flags,
        choices: Vec::new(),
        conditional: false,
    };
    let paths = battle
        .outro
        .as_ref()
        .and_then(|id| pack.scenes.get(id))
        .map_or_else(
            || vec![start.clone()],
            |s| scene_paths(pack, s, start.clone(), campaign, notes),
        );
    let mut queue = VecDeque::new();
    for node in &pack.campaign.nodes {
        if let Node::Battle {
            battle: id, next, ..
        } = node
        {
            if id == &battle.id {
                for path in &paths {
                    queue.push_back((next.clone(), path.clone()));
                }
            }
        }
    }
    let mut visited = BTreeSet::new();
    while let Some((id, path)) = queue.pop_front() {
        if !visited.insert((id.clone(), path.flags.clone(), path.conditional)) {
            continue;
        }
        if visited.len() > 2048 {
            break;
        }
        match pack.campaign.node(&id) {
            Some(Node::Drama { scene, next, .. }) => {
                let paths = pack.scenes.get(scene).map_or_else(
                    || vec![path.clone()],
                    |s| scene_paths(pack, s, path.clone(), campaign, notes),
                );
                for path in paths {
                    queue.push_back((next.clone(), path));
                }
            }
            Some(Node::Camp {
                battle: None, next, ..
            }) => queue.push_back((next.clone(), path)),
            Some(Node::Branch {
                flag,
                cmp,
                value,
                then,
                otherwise,
                ..
            }) => {
                let result = test_flag(&path.flags, flag, *cmp, *value);
                if result != Some(false) {
                    let mut p = path.clone();
                    p.conditional |= result.is_none();
                    queue.push_back((then.clone(), p));
                }
                if result != Some(true) {
                    let mut p = path;
                    p.conditional |= result.is_none();
                    queue.push_back((otherwise.clone(), p));
                }
            }
            // The next sortie belongs to the next guide, never this one.
            _ => {}
        }
    }
    notes.flush_post(pack);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::DirSource;
    use crate::script::parse_drama;
    use std::path::PathBuf;

    fn pack() -> Pack {
        Pack::load(&DirSource {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/base"),
        })
        .unwrap()
    }

    fn path() -> Path {
        Path {
            flags: BTreeMap::new(),
            choices: Vec::new(),
            conditional: false,
        }
    }

    #[test]
    fn choices_and_written_flags_control_rewards_without_mutating_campaign() {
        let pack = pack();
        let campaign = CampaignState::new_game(&pack);
        let before = campaign.clone();
        let scenes = parse_drama("test", "== rewards\n@choice\n  - accept -> accept\n  - refuse -> refuse\n@label accept\n@set accepted = 1\n@goto finish\n@label refuse\n@set accepted = 0\n@label finish\n@if accepted == 0 -> no_reward\n@item twin_swords\n@label no_reward\n@end\n").unwrap();
        let mut notes = Notes::default();
        let ends = scene_paths(&pack, &scenes[0], path(), &campaign, &mut notes);
        notes.flush_post(&pack);
        assert_eq!(ends.len(), 2);
        assert_eq!(notes.treasures.len(), 1);
        assert!(notes.treasures[0].contains("accept"));
        assert!(!notes.treasures[0].contains("refuse"));
        assert_eq!(campaign, before);
    }

    #[test]
    fn unknown_battle_flags_label_conditional_rewards_and_loops_are_bounded() {
        let pack = pack();
        let campaign = CampaignState::new_game(&pack);
        let scenes = parse_drama("test", "== conditional\n@if contact == 0 -> skip\n@item twin_swords\n@label skip\n@end\n== loop\n@label again\n@set n += 1\n@goto again\n").unwrap();
        let mut p = path();
        p.flags.insert("contact".into(), None);
        let mut notes = Notes::default();
        assert_eq!(
            scene_paths(&pack, &scenes[0], p, &campaign, &mut notes).len(),
            1
        );
        notes.flush_post(&pack);
        assert!(notes.treasures[0].contains("剧情条件"));
        assert!(scene_paths(&pack, &scenes[1], path(), &campaign, &mut notes).is_empty());
    }

    #[test]
    fn repeated_choices_show_one_short_reward_path() {
        let pack = pack();
        let campaign = CampaignState::new_game(&pack);
        let scene = parse_drama("test", "== loop\n@label menu\n@choice\n  - accept -> accepted\n  - leave -> finish\n@label accepted\n@item twin_swords\n@goto menu\n@label finish\n@end\n").unwrap().remove(0);
        let mut notes = Notes::default();
        scene_paths(&pack, &scene, path(), &campaign, &mut notes);
        notes.flush_post(&pack);
        assert_eq!(notes.treasures.len(), 1);
        assert_eq!(notes.treasures[0].matches("accept").count(), 1);
    }

    #[test]
    fn post_battle_search_stops_before_the_next_sortie() {
        let mut pack = pack();
        let campaign = CampaignState::new_game(&pack);
        let mut battle = pack.battles.values().next().unwrap().clone();
        battle.id = "current".into();
        battle.outro = None;
        battle.events.clear();
        let scene = parse_drama("test", "== future\n@item twin_swords\n@end\n")
            .unwrap()
            .remove(0);
        pack.scenes.insert(scene.id.clone(), scene);
        pack.campaign.nodes = vec![
            Node::Battle {
                id: "start".into(),
                battle: battle.id.clone(),
                next: "next_camp".into(),
                on_defeat: None,
            },
            Node::Camp {
                id: "next_camp".into(),
                title: String::new(),
                shop: Vec::new(),
                battle: Some("next".into()),
                next: "future".into(),
            },
            Node::Drama {
                id: "future".into(),
                scene: "future".into(),
                next: "future".into(),
            },
        ];
        let mut notes = Notes::default();
        after_battle(&pack, &battle, &campaign, &mut notes);
        assert!(notes.treasures.is_empty());
        if let Node::Camp { battle, .. } = &mut pack.campaign.nodes[1] {
            *battle = None;
        }
        after_battle(&pack, &battle, &campaign, &mut notes);
        assert_eq!(notes.treasures.len(), 1);
    }

    #[test]
    fn carried_enemy_equipment_is_not_a_reward() {
        let mut pack = pack();
        let campaign = CampaignState::new_game(&pack);
        let id = pack.battles.keys().next().unwrap().clone();
        let battle = pack.battles.get_mut(&id).unwrap();
        battle.outro = None;
        battle.events.clear();
        battle.treasures.clear();
        for u in &mut battle.units {
            u.drop = None;
            u.equip.get_or_insert_with(Default::default).weapon = Some("twin_swords".into());
        }
        pack.campaign.nodes.clear();
        let guide = battle_guide(&pack, &id, &campaign, &[]);
        assert!(guide[1].entries[0].starts_with("本关没有"));
    }

    #[test]
    fn coordinates_are_one_based_and_guides_are_read_only() {
        assert_eq!(position(crate::geom::Pos::new(0, 3)), "列1、行4");
        let pack = pack();
        let campaign = CampaignState::new_game(&pack);
        let before = campaign.clone();
        for id in pack.battles.keys() {
            let a = battle_guide(&pack, id, &campaign, &[]);
            assert_eq!(a.len(), 4);
            assert_eq!(a, battle_guide(&pack, id, &campaign, &[]));
        }
        assert_eq!(campaign, before);
    }
}
