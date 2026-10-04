//! Korean texts of the battle screen: phase and weather names, objectives, forecasts, result
//! lines and particle selection (이/가, 을/를, 은/는, 으로/로).

use hero_core::battle::{AttackForecast, DefeatReason, StrategyForecast, Weather};
use hero_core::battledef::{Condition, Side};

/// Whether the last Hangul syllable of `word` has a final consonant (받침). Non-Hangul endings
/// count as no 받침, except digits, which are read in Sino-Korean.
pub fn has_batchim(word: &str) -> bool {
    match word.trim_end().chars().last() {
        Some(c @ '\u{AC00}'..='\u{D7A3}') => (c as u32 - 0xAC00) % 28 != 0,
        // 영 일 이 삼 사 오 육 칠 팔 구: 받침 for 0, 1, 3, 6, 7, 8.
        Some(c @ '0'..='9') => matches!(c, '0' | '1' | '3' | '6' | '7' | '8'),
        _ => false,
    }
}

/// `word` followed by the particle that fits it: `with` after a 받침, `without` otherwise
/// (`josa("관우", "이", "가")` = `관우가`).
pub fn josa(word: &str, with: &str, without: &str) -> String {
    if word.chars().any(|c| ('\u{3400}'..='\u{9FFF}').contains(&c)) {
        return word.to_string();
    }
    let p = if has_batchim(word) { with } else { without };
    format!("{word}{p}")
}

/// Subject form: 관우가 / 화웅이.
pub fn subject(word: &str) -> String {
    josa(word, "이", "가")
}

/// Object form: 콩을 / 초열서를.
pub fn object(word: &str) -> String {
    josa(word, "을", "를")
}

pub fn side_name(side: Side) -> &'static str {
    match side {
        Side::Player => "我军",
        Side::Ally => "友军",
        Side::Enemy => "敌军",
    }
}

/// Phase banner title: `적군 페이즈`.
pub fn phase_title(side: Side) -> String {
    format!("{}行动", side_name(side))
}

/// Phase banner subtitle: `제 3턴 / 30`.
pub fn turn_text(turn: u32, limit: u32) -> String {
    format!("第 {turn} 回合 / {limit}")
}

pub fn weather_name(w: Weather) -> &'static str {
    match w {
        Weather::Clear => "晴",
        Weather::Cloudy => "阴",
        Weather::Rain => "雨",
    }
}

/// Icon key of a weather (`docs/ASSETS.md`).
pub fn weather_icon(w: Weather) -> &'static str {
    match w {
        Weather::Clear => "weather_clear",
        Weather::Cloudy => "weather_cloudy",
        Weather::Rain => "weather_rain",
    }
}

/// One condition of the objective window. `name` resolves a unit reference (tag or officer id)
/// to a display name.
pub fn condition_text(c: &Condition, name: impl Fn(&str) -> String) -> String {
    match c {
        Condition::DefeatAll => "敌军全灭".into(),
        Condition::DefeatUnit { target } => format!("击败 {}", name(target)),
        Condition::DefeatCommander => "击败敌军主将".into(),
        Condition::Reach {
            who,
            pos,
            radius,
            to,
        } => {
            let who = who.as_deref().map(&name).unwrap_or_else(|| "我军".into());
            let place = if let Some(to) = to {
                format!("({}, {})–({}, {}) 区域", pos.x, pos.y, to.x, to.y)
            } else if *radius > 0 {
                format!("({}, {}) 附近 {} 格内", pos.x, pos.y, radius)
            } else {
                format!("({}, {}) 位置", pos.x, pos.y)
            };
            format!("{} 到达 {place}", subject(&who))
        }
        Condition::SurviveTurns { turns } => format!("坚持 {turns} 回合"),
        Condition::UnitRetreated { target } => format!("{} 撤退", name(target)),
    }
}

/// Why a battle was lost, for the result window.
pub fn defeat_text(reason: DefeatReason, lord: Option<&str>) -> String {
    match reason {
        DefeatReason::LordRetreated => match lord {
            Some(l) => format!("{} 已撤退", subject(l)),
            None => "主将已撤退".into(),
        },
        DefeatReason::ArmyRetreated => "我军全部撤退".into(),
        DefeatReason::TurnLimit => "超过回合限制".into(),
        DefeatReason::Condition => "满足失败条件".into(),
        DefeatReason::Event => "战局不利".into(),
    }
}

/// Class affinity of an attack: `Some(true)` = advantage (▲), `Some(false)` = disadvantage (▼).
pub fn affinity_mark(affinity_pct: i32) -> Option<bool> {
    match affinity_pct {
        p if p < 100 => Some(true),
        p if p > 100 => Some(false),
        _ => None,
    }
}

/// Lines of the attack forecast window (without the affinity arrow, which is drawn).
pub struct AttackLines {
    /// `피해 210`
    pub damage: String,
    /// `병력 500 → 290` (or `→ 퇴각` when the hit defeats the target).
    pub result: String,
    /// `반격 61% · 피해 45` / `반격 없음`.
    pub counter: String,
    pub defeats: bool,
}

pub fn attack_lines(f: &AttackForecast, target_hp: i32) -> AttackLines {
    let left = (target_hp - f.damage).max(0);
    let defeats = left == 0;
    AttackLines {
        damage: format!("伤害 {}", f.damage),
        result: if defeats {
            format!("兵力 {target_hp} → 撤退")
        } else {
            format!("兵力 {target_hp} → {left}")
        },
        counter: match f.counter {
            Some(c) => format!("反击 {}% · 伤害 {}", c.chance, c.damage),
            None => "无反击".into(),
        },
        defeats,
    }
}

/// One line per affected unit in the strategy forecast: `명중 85% · 피해 230`, `회복 400`,
/// `명중 60%` (pure status/morale effects).
pub fn strategy_line(f: &StrategyForecast) -> String {
    let amount = if f.amount > 0 {
        Some(format!("伤害 {}", f.amount))
    } else if f.amount < 0 {
        Some(format!("恢复 {}", -f.amount))
    } else {
        None
    };
    match (f.chance >= 100, amount) {
        (true, Some(a)) => a,
        (true, None) => "成功 100%".into(),
        (false, Some(a)) => format!("命中 {}% · {a}", f.chance),
        (false, None) => format!("命中 {}%", f.chance),
    }
}

/// `사기 +20` / `사기 -23`.
pub fn morale_text(delta: i32) -> String {
    format!("士气 {delta:+}")
}

/// Element label used in the strategy list.
pub fn element_name(element: Option<&str>) -> &'static str {
    match element {
        Some("fire") => "火计",
        Some("water") => "水计",
        Some("earth") => "地计",
        Some(_) => "特殊",
        None => "",
    }
}

/// Banner text of a found treasure: `콩을 얻었다!` / `금 100을 얻었다!` /
/// `초열서와 금 50을 얻었다!`.
pub fn treasure_text(item: Option<&str>, gold: i64) -> String {
    match (item, gold) {
        (Some(i), g) if g > 0 => format!("{} 获得金 {g}！", josa(i, "과", "와")),
        (Some(i), _) => format!("获得了 {}！", object(i)),
        (None, g) => format!("获得金 {g}！"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_core::battle::CounterForecast;
    use hero_core::geom::Pos;

    #[test]
    fn particles() {
        assert_eq!(subject("관우"), "관우가");
        assert_eq!(subject("화웅"), "화웅이");
        assert_eq!(object("콩"), "콩을");
        assert_eq!(object("초열서"), "초열서를");
        assert_eq!(josa("金 100", "을", "를"), "金 100");
        assert_eq!(josa("金 2", "을", "를"), "金 2");
        assert_eq!(josa("Lu Bu", "이", "가"), "Lu Bu가");
        assert_eq!(subject("關羽"), "關羽");
        assert_eq!(object("豆"), "豆");
        assert!(!has_batchim(""));
    }

    #[test]
    fn names() {
        assert_eq!(phase_title(Side::Enemy), "敌军行动");
        assert_eq!(turn_text(3, 30), "第 3 回合 / 30");
        assert_eq!(weather_name(Weather::Rain), "雨");
        assert_eq!(weather_icon(Weather::Cloudy), "weather_cloudy");
        assert_eq!(element_name(Some("fire")), "火计");
        assert_eq!(element_name(None), "");
    }

    #[test]
    fn conditions() {
        let name = |r: &str| {
            if r == "hua_xiong" {
                "화웅".to_string()
            } else {
                r.to_string()
            }
        };
        assert_eq!(
            condition_text(
                &Condition::DefeatUnit {
                    target: "hua_xiong".into()
                },
                name
            ),
            "击败 화웅"
        );
        assert_eq!(condition_text(&Condition::DefeatAll, name), "敌军全灭");
        assert_eq!(
            condition_text(&Condition::SurviveTurns { turns: 10 }, name),
            "坚持 10 回合"
        );
        assert_eq!(
            condition_text(
                &Condition::Reach {
                    who: None,
                    pos: Pos::new(3, 4),
                    radius: 0,
                    to: None,
                },
                name
            ),
            "我军 到达 (3, 4) 位置"
        );
        assert_eq!(
            condition_text(
                &Condition::Reach {
                    who: Some("hua_xiong".into()),
                    pos: Pos::new(3, 4),
                    radius: 2,
                    to: None,
                },
                name
            ),
            "화웅이 到达 (3, 4) 附近 2 格内"
        );
        assert_eq!(
            condition_text(
                &Condition::Reach {
                    who: None,
                    pos: Pos::new(29, 10),
                    radius: 0,
                    to: Some(Pos::new(29, 14)),
                },
                name
            ),
            "我军 到达 (29, 10)–(29, 14) 区域"
        );
        assert_eq!(
            defeat_text(DefeatReason::LordRetreated, Some("유비")),
            "유비가 已撤退"
        );
        assert_eq!(defeat_text(DefeatReason::TurnLimit, None), "超过回合限制");
    }

    #[test]
    fn attack_forecast_lines() {
        let f = AttackForecast {
            damage: 210,
            affinity: 75,
            counter: Some(CounterForecast {
                damage: 45,
                chance: 61,
            }),
        };
        let l = attack_lines(&f, 500);
        assert_eq!(l.damage, "伤害 210");
        assert_eq!(l.result, "兵力 500 → 290");
        assert_eq!(l.counter, "反击 61% · 伤害 45");
        assert!(!l.defeats);
        assert_eq!(affinity_mark(f.affinity), Some(true));
        assert_eq!(affinity_mark(125), Some(false));
        assert_eq!(affinity_mark(100), None);
        let f = AttackForecast {
            damage: 900,
            affinity: 100,
            counter: None,
        };
        let l = attack_lines(&f, 500);
        assert_eq!(l.result, "兵力 500 → 撤退");
        assert_eq!(l.counter, "无反击");
        assert!(l.defeats);
    }

    #[test]
    fn strategy_forecast_lines() {
        let f = |chance, amount| StrategyForecast {
            unit: 0,
            chance,
            amount,
        };
        assert_eq!(strategy_line(&f(85, 230)), "命中 85% · 伤害 230");
        assert_eq!(strategy_line(&f(100, -400)), "恢复 400");
        assert_eq!(strategy_line(&f(60, 0)), "命中 60%");
        assert_eq!(strategy_line(&f(100, 0)), "成功 100%");
        assert_eq!(morale_text(20), "士气 +20");
        assert_eq!(morale_text(-23), "士气 -23");
    }

    #[test]
    fn treasures() {
        assert_eq!(treasure_text(Some("콩"), 0), "获得了 콩을！");
        assert_eq!(treasure_text(None, 100), "获得金 100！");
        assert_eq!(treasure_text(Some("초열서"), 50), "초열서와 获得金 50！");
        assert_eq!(treasure_text(Some("콩"), 50), "콩과 获得金 50！");
    }
}
