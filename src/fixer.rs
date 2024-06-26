// Simplified version of https://github.com/oxc-project/oxc/blob/crates_v0.13.3/crates/oxc_linter/src/fixer.rs
// MIT License (c) 2023 Boshen
//
// Our modifications:
// - Add Fix#trim_leading_whitespace

use std::borrow::Cow;

use oxc_span::Span;

#[derive(Debug, Clone, Default)]
pub struct Fix<'a> {
    pub content: Cow<'a, str>,
    pub span: Span,
    fixed: bool,
    trim_leading_whitespace: bool,
}

#[allow(unused)]
impl<'a> Fix<'a> {
    pub const fn delete(span: Span) -> Self {
        Self {
            content: Cow::Borrowed(""),
            span,
            fixed: false,
            trim_leading_whitespace: false,
        }
    }

    pub const fn delete_with_leading_whitespace(span: Span) -> Self {
        Self {
            content: Cow::Borrowed(""),
            span,
            fixed: false,
            trim_leading_whitespace: true,
        }
    }

    pub fn insert<T: Into<Cow<'a, str>>>(content: T, span: Span) -> Self {
        Self {
            content: content.into(),
            span,
            fixed: false,
            trim_leading_whitespace: false,
        }
    }
}

pub struct FixResult<'a> {
    pub fixed: bool,
    pub fixed_code: Cow<'a, str>,
    pub fixes: Vec<Fix<'a>>,
}

pub struct Fixer<'a> {
    source_text: &'a str,
    fixes: Vec<Fix<'a>>,
}

impl<'a> Fixer<'a> {
    pub fn new(source_text: &'a str, fixes: Vec<Fix<'a>>) -> Self {
        Self { source_text, fixes }
    }

    /// # Panics
    pub fn fix(mut self) -> FixResult<'a> {
        let source_text = self.source_text;

        self.fixes.sort_by_key(|m| m.span);
        let mut fixed = false;
        let mut output = String::with_capacity(source_text.len());
        let mut last_pos: i64 = -1;
        self.fixes.iter_mut().for_each(|fix| {
            let start = fix.span.start;
            let end = fix.span.end;
            if start > end {
                return;
            }
            if i64::from(start) <= last_pos {
                return;
            }

            let offset = usize::try_from(last_pos.max(0)).ok().unwrap();

            let start = if fix.trim_leading_whitespace {
                get_position_of_nearest_leading_newline(&source_text, start)
            } else {
                start
            };

            if start != fix.span.start {
                fix.span.start = start;
            }

            // Copy the text before the current fix
            output.push_str(&source_text[offset..start as usize]);
            // Apply the current fix
            output.push_str(&fix.content);

            last_pos = i64::from(end);

            fix.fixed = true;
            fixed = true;
        });

        // Copy the text after the last fix
        let offset = usize::try_from(last_pos.max(0)).ok().unwrap();
        output.push_str(&source_text[offset..]);

        let mut fixes = self
            .fixes
            .into_iter()
            .filter(|fix| !fix.fixed)
            .collect::<Vec<_>>();
        fixes.sort_by_key(|fix| (fix.span.start, fix.span.end));
        FixResult {
            fixed,
            fixed_code: Cow::Owned(output),
            fixes,
        }
    }
}

/// Find nearest \n before the given position
fn get_position_of_nearest_leading_newline(s: &str, starting_position: u32) -> u32 {
    let mut pos = starting_position;
    while pos > 0 {
        pos -= 1;
        if s.as_bytes()[pos as usize] == b'\n' {
            return pos;
        }
    }
    starting_position
}
