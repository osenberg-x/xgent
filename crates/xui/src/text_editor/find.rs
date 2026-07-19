//! 查找替换状态机。
//!
//! MVP 实现：存储查找串、替换串、匹配列表，提供 `find_all` / `replace_next` / `replace_all` API。
//! UI overlay（查找框）由调用方渲染，或由 `xui::TextEditor` 的简易渲染系统提供。
//!
//! 详见 `doc/design/editor-design.md` 2.3 节快捷键表（Cmd+F / Cmd+H）。

/// 单个匹配（字节区间，半开 [start, end)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindMatch {
    /// 起始字节偏移
    pub start: usize,
    /// 结束字节偏移（半开）
    pub end: usize,
}

/// 查找替换状态。
#[derive(Debug, Clone, Default)]
pub struct FindState {
    /// 查找串
    pub find: String,
    /// 替换串
    pub replace: String,
    /// 是否区分大小写（默认不区分）
    pub case_sensitive: bool,
    /// 当前激活的匹配下标（用户 Tab/Enter 循环）
    pub current: Option<usize>,
    /// 上次计算的匹配列表（由 `find_all` 填充）
    pub matches: Vec<FindMatch>,
    /// 是否处于查找模式（Cmd+F 激活）
    pub active: bool,
    /// 是否处于替换模式（Cmd+H 激活）
    pub replace_mode: bool,
}

impl FindState {
    /// 激活查找模式。
    pub fn open_find(&mut self) {
        self.active = true;
        self.replace_mode = false;
    }

    /// 激活查找替换模式。
    pub fn open_replace(&mut self) {
        self.active = true;
        self.replace_mode = true;
    }

    /// 关闭查找模式。
    pub fn close(&mut self) {
        self.active = false;
        self.replace_mode = false;
        self.matches.clear();
        self.current = None;
    }

    /// 在文本中查找所有匹配，填充 `matches`，返回匹配数。
    pub fn find_all(&mut self, text: &str) -> usize {
        self.matches.clear();
        self.current = None;
        if self.find.is_empty() {
            return 0;
        }
        let needle = self.find.as_str();
        let haystack = text;
        let mut start = 0usize;
        if self.case_sensitive {
            while let Some(pos) = haystack[start..].find(needle) {
                let abs = start + pos;
                self.matches.push(FindMatch {
                    start: abs,
                    end: abs + needle.len(),
                });
                start = abs + needle.len().max(1);
                if start >= haystack.len() {
                    break;
                }
            }
        } else {
            let needle_l = needle.to_lowercase();
            let hay_l = haystack.to_lowercase();
            while let Some(pos) = hay_l[start..].find(&needle_l) {
                let abs = start + pos;
                self.matches.push(FindMatch {
                    start: abs,
                    end: abs + needle.len(),
                });
                start = abs + needle.len().max(1);
                if start >= haystack.len() {
                    break;
                }
            }
        }
        if !self.matches.is_empty() {
            self.current = Some(0);
        }
        self.matches.len()
    }

    /// 循环到下一个匹配，返回其字节区间。
    pub fn next_match(&mut self) -> Option<FindMatch> {
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        let idx = match self.current {
            Some(i) => (i + 1) % len,
            None => 0,
        };
        self.current = Some(idx);
        Some(self.matches[idx])
    }

    /// 循环到上一个匹配，返回其字节区间。
    pub fn prev_match(&mut self) -> Option<FindMatch> {
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        let idx = match self.current {
            Some(i) => (i + len - 1) % len,
            None => len - 1,
        };
        self.current = Some(idx);
        Some(self.matches[idx])
    }

    /// 替换当前匹配，返回新文本与替换后的字节偏移（用于继续查找）。
    ///
    /// 调用方负责把新文本写回 `EditableText`。
    pub fn replace_current(&mut self, text: &str) -> Option<String> {
        let m = self.current?;
        let match_range = *self.matches.get(m)?;
        let mut out = String::with_capacity(text.len() + self.replace.len());
        out.push_str(&text[..match_range.start]);
        out.push_str(&self.replace);
        out.push_str(&text[match_range.end..]);
        Some(out)
    }

    /// 替换全部匹配，返回新文本。
    pub fn replace_all(&mut self, text: &str) -> Option<String> {
        if self.matches.is_empty() || self.find.is_empty() {
            return None;
        }
        // 从后向前替换，避免偏移失效
        let mut out = text.to_string();
        let mut to_replace: Vec<FindMatch> = self.matches.clone();
        to_replace.sort_by(|a, b| b.start.cmp(&a.start));
        for m in to_replace {
            out.replace_range(m.start..m.end, &self.replace);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_all_case_insensitive_default() {
        let mut f = FindState::default();
        f.find = "foo".into();
        let n = f.find_all("Foo bar foo FOO");
        assert_eq!(n, 3);
        assert_eq!(f.matches[0].start, 0);
        assert_eq!(f.matches[1].start, 8);
        assert_eq!(f.matches[2].start, 12);
    }

    #[test]
    fn find_all_case_sensitive() {
        let mut f = FindState::default();
        f.find = "foo".into();
        f.case_sensitive = true;
        let n = f.find_all("Foo bar foo FOO");
        assert_eq!(n, 1);
        assert_eq!(f.matches[0].start, 8);
    }

    #[test]
    fn find_all_empty_needle_returns_zero() {
        let mut f = FindState::default();
        f.find = "".into();
        assert_eq!(f.find_all("abc"), 0);
        assert!(f.matches.is_empty());
    }

    #[test]
    fn next_match_wraps_around() {
        let mut f = FindState::default();
        f.find = "a".into();
        f.find_all("a a a");
        // find_all 后 current = Some(0)（第一个匹配 start=0）
        // next_match 跳到下一个：(0+1)%3 = 1 → matches[1].start = 2
        let m1 = f.next_match().unwrap();
        assert_eq!(m1.start, 2);
        let m2 = f.next_match().unwrap();
        assert_eq!(m2.start, 4);
        // wrap
        let m3 = f.next_match().unwrap();
        assert_eq!(m3.start, 0);
    }

    #[test]
    fn prev_match_wraps_around() {
        let mut f = FindState::default();
        f.find = "a".into();
        f.find_all("a a a");
        let m = f.prev_match().unwrap();
        assert_eq!(m.start, 4); // 默认从最后一个的前一个
        let m2 = f.prev_match().unwrap();
        assert_eq!(m2.start, 2);
    }

    #[test]
    fn replace_current_returns_new_text() {
        let mut f = FindState::default();
        f.find = "foo".into();
        f.replace = "bar".into();
        f.find_all("foo baz foo");
        // current = 第一个 (start=0)
        let out = f.replace_current("foo baz foo").unwrap();
        assert_eq!(out, "bar baz foo");
    }

    #[test]
    fn replace_all_replaces_all() {
        let mut f = FindState::default();
        f.find = "foo".into();
        f.replace = "bar".into();
        f.find_all("foo foo foo");
        let out = f.replace_all("foo foo foo").unwrap();
        assert_eq!(out, "bar bar bar");
    }

    #[test]
    fn replace_all_empty_matches_returns_none() {
        let mut f = FindState::default();
        f.find = "xyz".into();
        f.replace = "bar".into();
        f.find_all("abc");
        assert!(f.replace_all("abc").is_none());
    }
}
