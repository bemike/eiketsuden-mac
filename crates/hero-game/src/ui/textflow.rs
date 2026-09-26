//! Paged typewriter text: pre-wrapped lines split into pages that are revealed character by
//! character. Pure logic (no drawing), shared by the drama dialogue box.
//!
//! ```ignore
//! let mut tw = Typewriter::new(ctx.gfx.wrap(text, FontId::Main, 1, width), 3);
//! tw.advance(chars_per_second * dt);
//! for line in tw.visible_lines() { /* draw */ }
//! if !tw.is_typing() && confirmed && !tw.next_page() { /* finished */ }
//! ```

/// See the module docs.
#[derive(Debug, Clone, PartialEq)]
pub struct Typewriter {
    pages: Vec<Vec<String>>,
    page: usize,
    /// Characters of the current page revealed so far (fractional while typing).
    shown: f32,
}

impl Typewriter {
    /// `lines` are already wrapped to the box width; `lines_per_page` is at least 1. Empty text
    /// still has one (empty) page.
    pub fn new(lines: Vec<String>, lines_per_page: usize) -> Typewriter {
        let per_page = lines_per_page.max(1);
        let mut pages: Vec<Vec<String>> = lines.chunks(per_page).map(<[String]>::to_vec).collect();
        if pages.is_empty() {
            pages.push(vec![String::new()]);
        }
        Typewriter {
            pages,
            page: 0,
            shown: 0.0,
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Index of the current page.
    pub fn page(&self) -> usize {
        self.page
    }

    /// Lines of the current page (complete, not only the revealed part).
    pub fn page_lines(&self) -> &[String] {
        &self.pages[self.page]
    }

    /// Number of characters on the current page.
    pub fn page_chars(&self) -> usize {
        self.page_lines().iter().map(|l| l.chars().count()).sum()
    }

    /// Characters of the current page revealed so far.
    pub fn shown_chars(&self) -> usize {
        (self.shown.max(0.0) as usize).min(self.page_chars())
    }

    /// The current page is still being revealed.
    pub fn is_typing(&self) -> bool {
        self.shown_chars() < self.page_chars()
    }

    /// Reveal `chars` more characters (negative or NaN amounts are ignored).
    pub fn advance(&mut self, chars: f32) {
        if chars.is_finite() && chars > 0.0 {
            self.shown = (self.shown + chars).min(self.page_chars() as f32);
        }
    }

    /// Reveal the rest of the current page.
    pub fn complete_page(&mut self) {
        self.shown = self.page_chars() as f32;
    }

    pub fn has_next_page(&self) -> bool {
        self.page + 1 < self.pages.len()
    }

    /// Turn to the next page (starting to type it); `false` when this was the last page.
    pub fn next_page(&mut self) -> bool {
        if self.has_next_page() {
            self.page += 1;
            self.shown = 0.0;
            true
        } else {
            false
        }
    }

    /// Revealed prefix of each line of the current page (lines not reached yet are omitted).
    pub fn visible_lines(&self) -> Vec<&str> {
        let mut remaining = self.shown_chars();
        let mut out = Vec::new();
        for line in self.page_lines() {
            if remaining == 0 {
                break;
            }
            let n = line.chars().count();
            if remaining >= n {
                out.push(line.as_str());
                remaining -= n;
            } else {
                let end = line
                    .char_indices()
                    .nth(remaining)
                    .map_or(line.len(), |(i, _)| i);
                out.push(&line[..end]);
                remaining = 0;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pages_and_typing() {
        let mut tw = Typewriter::new(lines(&["가나다", "라마", "바사", "아"]), 3);
        assert_eq!(tw.page_count(), 2);
        assert_eq!(tw.page_chars(), 7);
        assert!(tw.is_typing());
        assert!(tw.visible_lines().is_empty());

        tw.advance(4.5);
        assert_eq!(tw.visible_lines(), vec!["가나다", "라"]);
        tw.advance(-3.0);
        tw.advance(f32::NAN);
        assert_eq!(tw.shown_chars(), 4);

        tw.advance(100.0);
        assert!(!tw.is_typing());
        assert_eq!(tw.visible_lines(), vec!["가나다", "라마", "바사"]);

        assert!(tw.next_page());
        assert_eq!(tw.page(), 1);
        assert!(tw.is_typing());
        tw.complete_page();
        assert_eq!(tw.visible_lines(), vec!["아"]);
        assert!(!tw.next_page());
    }

    #[test]
    fn empty_text_has_one_empty_page() {
        let mut tw = Typewriter::new(Vec::new(), 3);
        assert_eq!(tw.page_count(), 1);
        assert!(!tw.is_typing());
        assert!(tw.visible_lines().is_empty());
        assert!(!tw.next_page());
        // A zero page size is treated as one line per page.
        let tw = Typewriter::new(lines(&["a", "b"]), 0);
        assert_eq!(tw.page_count(), 2);
    }
}
