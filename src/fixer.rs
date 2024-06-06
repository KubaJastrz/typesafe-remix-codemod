// Simplified version of https://github.com/oxc-project/oxc/blob/crates_v0.13.3/crates/oxc_linter/src/fixer.rs
// MIT License (c) 2023 Boshen

use std::borrow::Cow;

use oxc_span::Span;

#[derive(Debug, Clone, Default)]
pub struct Fix<'a> {
    pub content: Cow<'a, str>,
    pub span: Span,
    fixed: bool,
}

#[allow(unused)]
impl<'a> Fix<'a> {
    pub const fn delete(span: Span) -> Self {
        Self {
            content: Cow::Borrowed(""),
            span,
            fixed: false,
        }
    }

    pub fn insert<T: Into<Cow<'a, str>>>(content: T, span: Span) -> Self {
        Self {
            content: content.into(),
            span,
            fixed: false,
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

            fix.fixed = true;
            fixed = true;
            let offset = usize::try_from(last_pos.max(0)).ok().unwrap();
            output.push_str(&source_text[offset..start as usize]);
            output.push_str(&fix.content);
            last_pos = i64::from(end);
        });

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
