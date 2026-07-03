//! Post-transcription text cleanup (PRD 11.5).
//!
//! MVP implements `Raw` and `Basic`; `Smart` and LLM polish come later.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CleanupMode {
    /// Paste exact STT output.
    Raw,
    /// Trim whitespace, collapse duplicate spaces, fix spacing around punctuation.
    #[default]
    Basic,
}

impl std::str::FromStr for CleanupMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "raw" => Ok(Self::Raw),
            "basic" => Ok(Self::Basic),
            other => Err(format!("unknown cleanup mode: {other} (expected raw|basic)")),
        }
    }
}

pub fn clean(text: &str, mode: CleanupMode) -> String {
    match mode {
        CleanupMode::Raw => text.to_string(),
        CleanupMode::Basic => basic_clean(text),
    }
}

fn basic_clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = true; // leading spaces are dropped
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            // no space before closing punctuation
            if last_was_space && matches!(ch, '.' | ',' | '!' | '?' | ';' | ':') {
                if out.ends_with(' ') {
                    out.pop();
                }
            }
            out.push(ch);
            last_was_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_is_untouched() {
        assert_eq!(clean("  hi   there ", CleanupMode::Raw), "  hi   there ");
    }

    #[test]
    fn basic_collapses_whitespace() {
        assert_eq!(clean("  hi   there \n now ", CleanupMode::Basic), "hi there now");
    }

    #[test]
    fn basic_fixes_punctuation_spacing() {
        assert_eq!(clean("hello , world .", CleanupMode::Basic), "hello, world.");
    }
}
