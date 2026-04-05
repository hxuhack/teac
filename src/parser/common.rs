//! Common utilities for the TeaLang parser.
//!
//! This module contains the error type, the pest-derived parser struct, shared
//! type aliases, and small helper functions used across the other parser
//! sub-modules.

use pest_derive::Parser as DeriveParser;

/// Errors that can occur during parsing or code generation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A pest syntax error; the inner string carries the formatted pest message.
    #[error("{0}")]
    Syntax(String),

    /// An integer literal could not be parsed as `i32`.
    ///
    /// Carries the raw literal text and the source location for diagnostics.
    #[error("invalid integer literal `{literal}` at line {line}, column {column}")]
    InvalidNumber {
        literal: String,
        line: usize,
        column: usize,
        #[source]
        source: std::num::ParseIntError,
    },

    /// An I/O error occurred while writing output.
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    /// The pest parse tree had an unexpected structure.
    ///
    /// This indicates a bug in the grammar/parser rather than a user error.
    #[error("unexpected parse tree structure in {0}")]
    Grammar(String),
}

/// The pest-derived parser for TeaLang.
///
/// Generated automatically from `tealang.pest` via the `pest_derive` crate.
#[derive(DeriveParser)]
#[grammar = "tealang.pest"]
pub(crate) struct TeaLangParser;

/// Convenience `Result` alias used throughout the parser.
pub(crate) type ParseResult<T> = Result<T, Error>;

/// A single pest `Pair` node tagged with the TeaLang `Rule` type.
pub(crate) type Pair<'a> = pest::iterators::Pair<'a, Rule>;

/// Collapses a source snippet to a single line and truncates it to at most
/// 48 characters, appending `"..."` if truncated.
///
/// Used to produce compact near-context strings in error messages.
pub(crate) fn compact_snippet(snippet: &str) -> String {
    const MAX_CHARS: usize = 48;

    let compact = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = if compact.is_empty() {
        snippet.trim().to_string()
    } else {
        compact
    };

    if normalized.is_empty() {
        return "<empty>".to_string();
    }

    let mut chars = normalized.chars();
    let preview: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

/// Constructs an [`Error::Grammar`] that includes source location information
/// derived from `pair`'s span.
pub(crate) fn grammar_error(context: &'static str, pair: &Pair<'_>) -> Error {
    let span = pair.as_span();
    let (line, column) = span.start_pos().line_col();
    let near = compact_snippet(span.as_str());

    Error::Grammar(format!(
        "{context} at line {line}, column {column}, near `{near}`"
    ))
}

/// Constructs an [`Error::Grammar`] with a static message and no location
/// information.
pub(crate) fn grammar_error_static(context: &'static str) -> Error {
    Error::Grammar(context.to_string())
}

/// Returns the byte offset of `pair`'s start position within the source input.
pub(crate) fn get_pos(pair: &Pair<'_>) -> usize {
    pair.as_span().start()
}

/// Parses the text of `pair` as a decimal `i32`.
///
/// Returns [`Error::InvalidNumber`] with location information on failure.
pub(crate) fn parse_num(pair: Pair) -> ParseResult<i32> {
    let literal = pair.as_str().to_string();
    let (line, column) = pair.as_span().start_pos().line_col();

    literal.parse().map_err(|source| Error::InvalidNumber {
        literal,
        line,
        column,
        source,
    })
}
