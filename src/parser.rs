//! Functions for parsing arguments.
#![allow(dead_code)]

use std::str::pattern::{Pattern, ReverseSearcher};

use crate::commands::{CommandError, CommandResult};
use crate::utils::{self, consts};

/// Returns `Some((prefix, unprefixed))`,
/// where `prefix` is the matched prefix and `unprefixed` is everything after.
/// Otherwise, returns `None` if no prefix was matched from `prefixes`.
pub fn unprefix_with<I, T>(prefixes: I, text: &str) -> Option<(&str, &str)>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    for prefix in prefixes {
        let prefix = prefix.as_ref();
        let stripped = text.strip_prefix(prefix);

        if let Some(stripped) = stripped {
            return Some((&text[..prefix.len()], stripped));
        }
    }

    None
}

/// Returns a tuple of `(next, rest)`, where `next` is the part before any whitespaces and `rest` is everything after any whitespaces.
pub fn split_once_whitespace(text: &str) -> (&str, Option<&str>) {
    text.split_once(char::is_whitespace)
        .map_or((text, None), |(n, r)| (n, Some(r)))
    // .unwrap_or((text, ""))
}

/// Try to parse string-slice into arg parts.
/// For more details about individual argument parsing, see [`maybe_quoted_arg`](maybe_quoted_arg)
pub fn parse_args(mut input: &str) -> Result<Vec<&str>, CommandError> {
    let mut args = Vec::new();

    loop {
        match maybe_quoted_arg(input) {
            Ok((arg, Some(rest))) => {
                input = rest;
                args.push(arg);
            },
            Ok((arg, None)) => {
                args.push(arg);
                break;
            },
            Err(CommandError::MissingArgs) => break, // No more args to parse.
            Err(e) => return Err(e),                 // Return if failed to parse.
        }
    }

    Ok(args)
}

/// Parse text and return a tuple `(arg, Option<rest>)`,
/// where `arg` is either the first quoted part, the first whitespace separated part
/// or the whole input (after `trim_start`).
/// The `Option` will contain the remaining text, if any.
/// # Notes
/// - Escape characters are **not** handled.
/// - If a non-quoted argument contains any delimiters before any whitespace,
/// those characters (and everything upto a whitespace or the end) will be in the `arg`.
/// - If a quoted argument is followed by any character (whitespace or not),
/// those characters will be in the remaining `Option`.
pub fn maybe_quoted_arg(input: &str) -> Result<(&str, Option<&str>), CommandError> {
    // First trim off any leading whitespace.
    let input = input.trim_start();

    // Indexing a string is in bytes, so enumerate the bytes.
    let mut bytes = input.bytes().enumerate();

    // Get the first byte or return an error for a missing argument.
    let (_, initial) = bytes.next().ok_or(CommandError::MissingArgs)?;

    // Check if the first byte is a delimiter character (assuming all delimiter characters are one byte wide utf-8).
    if consts::DELIMITERS.contains(&(initial as char)) {
        // Find the matching pair.
        let idx = loop {
            let (idx, _) = bytes.find(|(_, c)| &initial == c).ok_or_else(|| {
                let input = utils::escape_discord_chars(input);
                CommandError::ParseError(format!(
                    "Missing matching delimiter: '{input}', expected one of: {}.",
                    utils::nice_list(consts::DELIMITERS)
                ))
            })?;

            // Make sure the byte is not part of another character, else try again.
            if input.is_char_boundary(idx) {
                break idx;
            }
        };

        // Return everything between the two and then everything after, if any.
        Ok((&input[1..idx], input.get(idx + 1..)))
    } else {
        // Did not start with a delimiter, try to split by whitespace instead.
        Ok(split_once_whitespace(input))
    }
}

/// Returns a string-slice without delimiters, or returns ´input´ if no delimiters are found or can be stripped.
pub fn strip_delimits<'a, P>(input: &'a str, delimits: P) -> &'a str
where
    P: Pattern<'a, Searcher: ReverseSearcher<'a>> + Copy,
{
    match is_surrounded_by(input, delimits) {
        Some(b) => {
            if b {
                input
                    .strip_prefix(delimits)
                    .and_then(|s| s.strip_suffix(delimits))
                    .unwrap_or(input)
            } else {
                input
            }
        },
        None => input,
    }
}

/// Returns `Some(true)` if `target` is surrounded by any matching pair of delimiters.
/// Returns `None` if `target` is too short (including empty).
pub fn is_surrounded_by<'a, P>(target: &'a str, delimits: P) -> Option<bool>
where
    P: Pattern<'a, Searcher: ReverseSearcher<'a>> + Copy,
{
    let mut chars = target.chars();
    let left = chars.next()?; // None, if empty.
    let right = chars.last()?; // None, if too short.

    Some(left == right && target.starts_with(delimits) && target.ends_with(delimits))
}

/// Make sure there's nothing else by mistake.
pub fn ensure_rest_is_empty(rest: Option<&str>) -> CommandResult {
    if let Some(rest) = rest {
        if !rest.trim().is_empty() {
            return Err(CommandError::UnexpectedArgs(
                format!("Unexpected '{rest}'",),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn overly_ugly_arguments() {
        let s = r#"    foo    bar "baz\n    `.-_' thing" abc-goo'`" "sample text \\\"* ;    "#;
        assert_eq!(
            parse_args(s),
            Ok(vec![
                r#"foo"#,
                r#"bar"#,
                r#"baz\n    `.-_' thing"#,
                r#"abc-goo'`""#,
                r#"sample text \\\"#,
                r#"*"#,
                r#";"#,
            ])
        );
    }

    #[test]
    fn empty_arguments() {
        let s = "";
        assert_eq!(parse_args(s), Ok(vec![]));

        let s = "  \t  ";
        assert_eq!(parse_args(s), Ok(vec![]));
    }

    #[test]
    fn parse_one_arg() {
        let s = r#"    foo    bar"#;
        assert_eq!(maybe_quoted_arg(s), Ok(("foo", Some(r#"   bar"#))));

        let s = r#"foo bar"#;
        assert_eq!(maybe_quoted_arg(s), Ok(("foo", Some(r#"bar"#))));

        let s = r#"    "foo"bar "#;
        assert_eq!(maybe_quoted_arg(s), Ok(("foo", Some(r#"bar "#))));

        let s = r#""foo" bar "#;
        assert_eq!(maybe_quoted_arg(s), Ok(("foo", Some(r#" bar "#))));
    }
}
