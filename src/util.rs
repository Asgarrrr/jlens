use unicode_width::UnicodeWidthStr;

/// Truncate a string to at most `max_chars` characters, respecting char boundaries.
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Truncate a string to fit within `max_width` terminal columns,
/// respecting Unicode display widths.
#[allow(dead_code)]
pub fn truncate_display_width(s: &str, max_width: usize) -> &str {
    let mut width = 0;
    for (idx, ch) in s.char_indices() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            return &s[..idx];
        }
        width += ch_width;
    }
    s
}

/// Measure the display width of a string in terminal columns.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Count the number of decimal digits in a number using integer arithmetic.
pub fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut count = 0;
    let mut val = n;
    while val > 0 {
        val /= 10;
        count += 1;
    }
    count
}

/// Reusable vertical scroll state — eliminates duplicated navigation logic across views.
pub struct ScrollState {
    pub selected: usize,
    pub offset: usize,
    pub viewport: usize,
}

impl ScrollState {
    pub fn new() -> Self {
        Self { selected: 0, offset: 0, viewport: 0 }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.ensure_visible();
    }

    pub fn move_down(&mut self, total: usize) {
        if total > 0 {
            self.selected = (self.selected + 1).min(total - 1);
        }
        self.ensure_visible();
    }

    pub fn page_up(&mut self, margin: usize) {
        let jump = self.viewport.saturating_sub(margin);
        self.selected = self.selected.saturating_sub(jump);
        self.ensure_visible();
    }

    pub fn page_down(&mut self, total: usize, margin: usize) {
        if total > 0 {
            let jump = self.viewport.saturating_sub(margin);
            self.selected = (self.selected + jump).min(total - 1);
        }
        self.ensure_visible();
    }

    pub fn go_top(&mut self) {
        self.selected = 0;
        self.ensure_visible();
    }

    pub fn go_bottom(&mut self, total: usize) {
        if total > 0 {
            self.selected = total - 1;
        }
        self.ensure_visible();
    }

    /// Clamp selected within bounds and adjust scroll offset.
    pub fn clamp(&mut self, total: usize) {
        if total > 0 {
            self.selected = self.selected.min(total - 1);
        } else {
            self.selected = 0;
        }
        self.ensure_visible();
    }

    pub fn ensure_visible(&mut self) {
        if self.viewport == 0 {
            return;
        }
        if self.selected < self.offset {
            self.offset = self.selected;
        }
        if self.selected >= self.offset + self.viewport {
            self.offset = self.selected - self.viewport + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
        assert_eq!(truncate_chars("short", 10), "short");
    }

    #[test]
    fn truncate_unicode() {
        assert_eq!(truncate_chars("café latte", 4), "café");
        // Emoji: each is a single char but multiple bytes
        assert_eq!(truncate_chars("🎉🎊🎈🎁", 2), "🎉🎊");
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate_chars("", 5), "");
    }

    #[test]
    fn display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
    }

    #[test]
    fn display_width_cjk() {
        // CJK characters are 2 columns wide
        assert_eq!(display_width("漢字"), 4);
    }

    #[test]
    fn digit_count_values() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(99), 2);
        assert_eq!(digit_count(100), 3);
        assert_eq!(digit_count(999), 3);
        assert_eq!(digit_count(1000), 4);
        assert_eq!(digit_count(999_999_999), 9);
    }

    #[test]
    fn scroll_state_navigation() {
        let mut s = ScrollState::new();
        s.viewport = 10;
        s.move_down(100);
        assert_eq!(s.selected, 1);
        s.page_down(100, 2);
        assert_eq!(s.selected, 9);
        s.go_bottom(100);
        assert_eq!(s.selected, 99);
        s.go_top();
        assert_eq!(s.selected, 0);
    }
}
