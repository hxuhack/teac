//! Statement-related parsing for TeaLang.
//!
//! This module handles code-block statements: variable declarations,
//! assignments, function-call statements, and control-flow constructs
//! (`if`/`else`, `while`, `return`, `continue`, `break`, `null`).

use crate::ast;

use super::ParseContext;
use super::common::{ParseResult, Pair, Rule, get_pos, grammar_error};

impl<'a> ParseContext<'a> {
    /// Parses a single code-block statement.
    ///
    /// Inspects the inner pest rule and dispatches to the appropriate
    /// sub-parser, covering:
    /// `var_decl_stmt`, `assignment_stmt`, `call_stmt`, `if_stmt`,
    /// `while_stmt`, `return_stmt`, `continue_stmt`, `break_stmt`, `null_stmt`.
    pub(crate) fn parse_code_block_stmt(&self, pair: Pair) -> ParseResult<Box<ast::CodeBlockStmt>> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::var_decl_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::VarDecl(self.parse_var_decl_stmt(inner)?),
                    }));
                }
                Rule::assignment_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::Assignment(
                            self.parse_assignment_stmt(inner)?,
                        ),
                    }));
                }
                Rule::call_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::Call(self.parse_call_stmt(inner)?),
                    }));
                }
                Rule::if_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::If(self.parse_if_stmt(inner)?),
                    }));
                }
                Rule::while_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::While(self.parse_while_stmt(inner)?),
                    }));
                }
                Rule::return_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::Return(self.parse_return_stmt(inner)?),
                    }));
                }
                Rule::continue_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::Continue(Box::new(ast::ContinueStmt {})),
                    }));
                }
                Rule::break_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::Break(Box::new(ast::BreakStmt {})),
                    }));
                }
                Rule::null_stmt => {
                    return Ok(Box::new(ast::CodeBlockStmt {
                        inner: ast::CodeBlockStmtInner::Null(Box::new(ast::NullStmt {})),
                    }));
                }
                _ => {}
            }
        }

        Err(grammar_error("code_block_stmt", &pair_for_error))
    }

    /// Parses an assignment statement of the form `left_val = right_val;`.
    fn parse_assignment_stmt(&self, pair: Pair) -> ParseResult<Box<ast::AssignmentStmt>> {
        let pair_for_error = pair.clone();
        let mut left_val = None;
        let mut right_val = None;

        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::left_val => left_val = Some(self.parse_left_val(inner)?),
                Rule::right_val => right_val = Some(self.parse_right_val(inner)?),
                _ => {}
            }
        }

        Ok(Box::new(ast::AssignmentStmt {
            left_val: left_val
                .ok_or_else(|| grammar_error("assignment.left_val", &pair_for_error))?,
            right_val: right_val
                .ok_or_else(|| grammar_error("assignment.right_val", &pair_for_error))?,
        }))
    }

    /// Parses a function-call statement (a function call used as a statement,
    /// discarding the return value).
    fn parse_call_stmt(&self, pair: Pair) -> ParseResult<Box<ast::CallStmt>> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            if inner.as_rule() == Rule::fn_call {
                return Ok(Box::new(ast::CallStmt {
                    fn_call: self.parse_fn_call(inner)?,
                }));
            }
        }

        Err(grammar_error("call_stmt", &pair_for_error))
    }

    /// Parses a `return` statement with an optional return value.
    fn parse_return_stmt(&self, pair: Pair) -> ParseResult<Box<ast::ReturnStmt>> {
        let mut val = None;

        for inner in pair.into_inner() {
            if inner.as_rule() == Rule::right_val {
                val = Some(self.parse_right_val(inner)?);
            }
        }

        Ok(Box::new(ast::ReturnStmt { val }))
    }

    /// Parses an `if`/`else` statement.
    ///
    /// Collects the boolean condition, the if-branch statement list, and an
    /// optional else-branch statement list.
    fn parse_if_stmt(&self, pair: Pair) -> ParseResult<Box<ast::IfStmt>> {
        let pair_for_error = pair.clone();
        let mut bool_unit = None;
        let mut if_stmts = Vec::new();
        let mut else_stmts = None;
        let mut in_else = false;

        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::bool_expr => {
                    let pos = get_pos(&inner);
                    let bool_expr = self.parse_bool_expr(inner)?;
                    bool_unit = Some(Box::new(ast::BoolUnit {
                        pos,
                        inner: ast::BoolUnitInner::BoolExpr(bool_expr),
                    }));
                }
                Rule::code_block_stmt => {
                    if in_else {
                        let else_branch = else_stmts.get_or_insert_with(Vec::new);
                        else_branch.push(*self.parse_code_block_stmt(inner)?);
                    } else {
                        if_stmts.push(*self.parse_code_block_stmt(inner)?);
                    }
                }
                Rule::kw_else => {
                    in_else = true;
                }
                _ => {}
            }
        }

        Ok(Box::new(ast::IfStmt {
            bool_unit: bool_unit.ok_or_else(|| grammar_error("cond.bool_unit", &pair_for_error))?,
            if_stmts,
            else_stmts,
        }))
    }

    /// Parses a `while` loop statement.
    ///
    /// Collects the boolean loop condition and the loop-body statement list.
    fn parse_while_stmt(&self, pair: Pair) -> ParseResult<Box<ast::WhileStmt>> {
        let pair_for_error = pair.clone();
        let mut bool_unit = None;
        let mut stmts = Vec::new();

        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::bool_expr => {
                    let pos = get_pos(&inner);
                    let bool_expr = self.parse_bool_expr(inner)?;
                    bool_unit = Some(Box::new(ast::BoolUnit {
                        pos,
                        inner: ast::BoolUnitInner::BoolExpr(bool_expr),
                    }));
                }
                Rule::code_block_stmt => {
                    stmts.push(*self.parse_code_block_stmt(inner)?);
                }
                _ => {}
            }
        }

        Ok(Box::new(ast::WhileStmt {
            bool_unit: bool_unit
                .ok_or_else(|| grammar_error("cond.bool_unit", &pair_for_error))?,
            stmts,
        }))
    }
}
