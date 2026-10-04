//! Korean particles (조사) whose form depends on the word before them.
//!
//! `관우` + 을/를 → `관우를`, `초열서` + 을/를 → `초열서를`, `장창` + 을/를 → `장창을`. The choice
//! depends on whether the last syllable has a final consonant (받침). Digits are read the Korean way
//! (`3` = 삼 → `3을`). For anything else (Latin letters, symbols) the neutral combined form
//! `을(를)` is used, which is always correct to read.

/// A particle with a form after a final consonant and a form after a vowel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Particle {
    /// 을 / 를 (object).
    EulReul,
    /// 이 / 가 (subject).
    IGa,
    /// 은 / 는 (topic).
    EunNeun,
    /// 과 / 와 (and, with).
    GwaWa,
    /// 으로 / 로 (towards, into); a final ㄹ takes 로.
    EuroRo,
}

impl Particle {
    /// `(after a final consonant, after a vowel)`.
    fn forms(self) -> (&'static str, &'static str) {
        match self {
            Particle::EulReul => ("을", "를"),
            Particle::IGa => ("이", "가"),
            Particle::EunNeun => ("은", "는"),
            Particle::GwaWa => ("과", "와"),
            Particle::EuroRo => ("으로", "로"),
        }
    }
}

/// Final consonant of the last character of `word`: `Some(Some(jong))` for a Hangul syllable or
/// digit with a final consonant (index 1..=27 of the Unicode jongseong table), `Some(None)` for
/// one that ends in a vowel, `None` when it cannot be decided.
fn final_consonant(word: &str) -> Option<Option<u32>> {
    let c = word.trim_end().chars().last()?;
    let code = c as u32;
    if (0xAC00..=0xD7A3).contains(&code) {
        let jong = (code - 0xAC00) % 28;
        return Some((jong != 0).then_some(jong));
    }
    // Korean readings of the digits: 영 일 이 삼 사 오 육 칠 팔 구.
    const RIEUL: u32 = 8;
    const MIEUM: u32 = 16;
    const IEUNG: u32 = 21;
    match c {
        '0' => Some(Some(IEUNG)),
        '1' | '7' | '8' => Some(Some(RIEUL)),
        '3' => Some(Some(MIEUM)),
        '6' => Some(Some(1)), // 육: ㄱ
        '2' | '4' | '5' | '9' => Some(None),
        _ => None,
    }
}

/// The particle form that follows `word` (without the word).
pub fn particle(word: &str, p: Particle) -> String {
    const RIEUL: u32 = 8;
    let (after_consonant, after_vowel) = p.forms();
    match final_consonant(word) {
        Some(Some(RIEUL)) if p == Particle::EuroRo => after_vowel.to_string(),
        Some(Some(_)) => after_consonant.to_string(),
        Some(None) => after_vowel.to_string(),
        None if p == Particle::EuroRo => "(으)로".to_string(),
        None => format!("{after_consonant}({after_vowel})"),
    }
}

/// `word` followed by the matching particle, e.g. `with_particle("관우", Particle::EulReul)` →
/// `관우를`.
pub fn with_particle(word: &str, p: Particle) -> String {
    // Chinese names have no Korean grammatical suffix. Preserve the underlying Korean
    // helper for its original callers and tests, but never append particles to Chinese UI.
    if word.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)) {
        return word.to_string();
    }
    format!("{word}{}", particle(word, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_names_do_not_get_korean_particles() {
        for p in [
            Particle::EulReul,
            Particle::IGa,
            Particle::EunNeun,
            Particle::GwaWa,
            Particle::EuroRo,
        ] {
            assert_eq!(with_particle("劉備", p), "劉備");
            assert_eq!(with_particle("军资金 500", p), "军资金 500");
        }
    }

    #[test]
    fn picks_the_form_by_final_consonant() {
        assert_eq!(with_particle("관우", Particle::EulReul), "관우를");
        assert_eq!(with_particle("장창", Particle::EulReul), "장창을");
        assert_eq!(with_particle("초열서", Particle::IGa), "초열서가");
        assert_eq!(with_particle("유비", Particle::EunNeun), "유비는");
        assert_eq!(with_particle("장비", Particle::GwaWa), "장비와");
        assert_eq!(with_particle("조조", Particle::GwaWa), "조조와");
        assert_eq!(with_particle("관평", Particle::GwaWa), "관평과");
    }

    #[test]
    fn rieul_takes_ro() {
        assert_eq!(with_particle("중기병", Particle::EuroRo), "중기병으로");
        assert_eq!(with_particle("궁술", Particle::EuroRo), "궁술로");
        assert_eq!(with_particle("단병", Particle::EuroRo), "단병으로");
        assert_eq!(with_particle("수송대", Particle::EuroRo), "수송대로");
    }

    #[test]
    fn digits_and_unknown_endings() {
        assert_eq!(with_particle("금 500", Particle::EulReul), "금 500을");
        assert_eq!(with_particle("3", Particle::IGa), "3이");
        assert_eq!(with_particle("2", Particle::IGa), "2가");
        assert_eq!(with_particle("Lv1", Particle::EuroRo), "Lv1로");
        assert_eq!(with_particle("abc", Particle::EulReul), "abc을(를)");
        assert_eq!(with_particle("abc", Particle::EuroRo), "abc(으)로");
        assert_eq!(with_particle("", Particle::IGa), "이(가)");
    }
}
