//! Entry point for the TeaLang parser module.
//!
//! This module is responsible for parsing TeaLang source code (a `&str`) into
//! an Abstract Syntax Tree ([`ast::Program`]).  It exposes a [`Parser`] struct
//! that implements the [`Generator`] trait, and an internal [`ParseContext`]
//! that drives the pest-based parse tree traversal.

mod common;
mod decl;
mod expr;
mod stmt;

use std::io::Write;

use pest::Parser as PestParser;

use crate::ast;
use crate::common::Generator;

pub use self::common::Error;
use self::common::{grammar_error_static, ParseResult, Rule, TeaLangParser};

/// The top-level TeaLang parser.
///
/// Holds a reference to the source string and, after [`Generator::generate`]
/// is called, the resulting [`ast::Program`] boxed AST.
pub struct Parser<'a> {
    input: &'a str,
    pub program: Option<Box<ast::Program>>,
}

impl<'a> Parser<'a> {
    /// Creates a new `Parser` for the given source string.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            program: None,
        }
    }
}

impl<'a> Generator for Parser<'a> {
    type Error = Error;

    /// Runs the pest parser on the stored input and stores the resulting AST
    /// in `self.program`.
    fn generate(&mut self) -> Result<(), Error> {
        let ctx = ParseContext::new(self.input);
        self.program = Some(ctx.parse()?);
        Ok(())
    }

    /// Writes the textual representation of the parsed AST to `w`.
    ///
    /// Returns `Error::Grammar` if called before [`generate`](Self::generate).
    fn output<W: Write>(&self, w: &mut W) -> Result<(), Error> {
        let ast = self
            .program
            .as_ref()
            .ok_or_else(|| grammar_error_static("output before generate"))?;
        write!(w, "{ast}")?;
        Ok(())
    }
}

/// Parse-time context that carries a reference to the original source string.
///
/// Created internally by [`Parser::generate`] and used throughout the parse
/// tree traversal to produce AST nodes.
pub(crate) struct ParseContext<'a> {
    #[allow(dead_code)]
    input: &'a str,
}

impl<'a> ParseContext<'a> {
    fn new(input: &'a str) -> Self {
        Self { input }
    }

    /// Top-level parse entry point.
    ///
    /// Invokes pest to parse the `program` rule, then walks the resulting pair
    /// tree collecting `use_stmt` and `program_element` nodes into an
    /// [`ast::Program`].
    fn parse(&self) -> ParseResult<Box<ast::Program>> {
        let pairs = <TeaLangParser as PestParser<Rule>>::parse(Rule::program, self.input)
            .map_err(|e| Error::Syntax(e.to_string()))?;

        let mut use_stmts = Vec::new();
        let mut elements = Vec::new();

        for pair in pairs {
            if pair.as_rule() == Rule::program {
                for inner in pair.into_inner() {
                    match inner.as_rule() {
                        Rule::use_stmt => {
                            use_stmts.push(self.parse_use_stmt(inner)?);
                        }
                        Rule::program_element => {
                            if let Some(elem) = self.parse_program_element(inner)? {
                                elements.push(*elem);
                            }
                        }
                        Rule::EOI => {}
                        _ => {}
                    }
                }
            }
        }

        Ok(Box::new(ast::Program {
            use_stmts,
            elements,
        }))
    }
}
