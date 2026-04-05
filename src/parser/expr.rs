//! Expression-related parsing for TeaLang.
//!
//! This module handles arithmetic expressions, boolean expressions, comparison
//! expressions, function calls, and left-value / right-value parsing.

use crate::ast;

use super::common::{get_pos, grammar_error, parse_num, Pair, ParseResult, Rule};
use super::ParseContext;

impl<'a> ParseContext<'a> {
    /// Parses a comma-separated list of right values.
    pub(crate) fn parse_right_val_list(&self, pair: Pair) -> ParseResult<Vec<ast::RightVal>> {
        let mut vals = Vec::new();
        for inner in pair.into_inner() {
            if inner.as_rule() == Rule::right_val {
                vals.push(*self.parse_right_val(inner)?);
            }
        }
        Ok(vals)
    }

    /// Parses a single right value, which is either a boolean expression or an
    /// arithmetic expression.
    pub(crate) fn parse_right_val(&self, pair: Pair) -> ParseResult<Box<ast::RightVal>> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::bool_expr => {
                    return Ok(Box::new(ast::RightVal {
                        inner: ast::RightValInner::BoolExpr(self.parse_bool_expr(inner)?),
                    }));
                }
                Rule::arith_expr => {
                    return Ok(Box::new(ast::RightVal {
                        inner: ast::RightValInner::ArithExpr(self.parse_arith_expr(inner)?),
                    }));
                }
                _ => {}
            }
        }

        Err(grammar_error("right_val", &pair_for_error))
    }

    /// Parses a boolean expression, handling `||` as a left-associative binary
    /// operator over boolean AND-terms.
    pub(crate) fn parse_bool_expr(&self, pair: Pair) -> ParseResult<Box<ast::BoolExpr>> {
        let pair_for_error = pair.clone();
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        if inner_pairs.is_empty() {
            return Err(grammar_error("bool_expr", &pair_for_error));
        }

        let mut expr = self.parse_bool_and_term(inner_pairs[0].clone())?;

        let mut i = 1;
        while i < inner_pairs.len() {
            if inner_pairs[i].as_rule() == Rule::op_or {
                let right = self.parse_bool_and_term(inner_pairs[i + 1].clone())?;
                expr = Box::new(ast::BoolExpr {
                    pos: expr.pos,
                    inner: ast::BoolExprInner::BoolBiOpExpr(Box::new(ast::BoolBiOpExpr {
                        op: ast::BoolBiOp::Or,
                        left: expr,
                        right,
                    })),
                });
                i += 2;
            } else {
                i += 1;
            }
        }

        Ok(expr)
    }

    /// Parses a boolean AND-term, handling `&&` as a left-associative binary
    /// operator over boolean unit atoms.
    fn parse_bool_and_term(&self, pair: Pair) -> ParseResult<Box<ast::BoolExpr>> {
        let pair_for_error = pair.clone();
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        if inner_pairs.is_empty() {
            return Err(grammar_error("bool_and_term", &pair_for_error));
        }

        let first_unit = self.parse_bool_unit_atom(inner_pairs[0].clone())?;
        let mut expr = Box::new(ast::BoolExpr {
            pos: first_unit.pos,
            inner: ast::BoolExprInner::BoolUnit(first_unit),
        });

        let mut i = 1;
        while i < inner_pairs.len() {
            if inner_pairs[i].as_rule() == Rule::op_and {
                let right_unit = self.parse_bool_unit_atom(inner_pairs[i + 1].clone())?;
                let right_expr = Box::new(ast::BoolExpr {
                    pos: right_unit.pos,
                    inner: ast::BoolExprInner::BoolUnit(right_unit),
                });

                expr = Box::new(ast::BoolExpr {
                    pos: expr.pos,
                    inner: ast::BoolExprInner::BoolBiOpExpr(Box::new(ast::BoolBiOpExpr {
                        op: ast::BoolBiOp::And,
                        left: expr,
                        right: right_expr,
                    })),
                });
                i += 2;
            } else {
                i += 1;
            }
        }

        Ok(expr)
    }

    /// Parses a boolean unit atom.
    ///
    /// Handles:
    /// - `!<atom>` (logical NOT)
    /// - A parenthesised sub-expression (`bool_unit_paren`)
    /// - A direct comparison expression (`bool_comparison`)
    fn parse_bool_unit_atom(&self, pair: Pair) -> ParseResult<Box<ast::BoolUnit>> {
        let pair_for_error = pair.clone();
        let pos = get_pos(&pair);
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        if inner_pairs.len() == 2 && inner_pairs[0].as_rule() == Rule::op_not {
            let cond = self.parse_bool_unit_atom(inner_pairs[1].clone())?;
            return Ok(Box::new(ast::BoolUnit {
                pos,
                inner: ast::BoolUnitInner::BoolUOpExpr(Box::new(ast::BoolUOpExpr {
                    op: ast::BoolUOp::Not,
                    cond,
                })),
            }));
        }

        for inner in inner_pairs {
            match inner.as_rule() {
                Rule::bool_unit_paren => {
                    return self.parse_bool_unit_paren(inner);
                }
                Rule::bool_comparison => {
                    return self.parse_bool_comparison(inner);
                }
                _ => {}
            }
        }

        Err(grammar_error("bool_unit_atom", &pair_for_error))
    }

    /// Parses a parenthesised boolean unit.
    ///
    /// The content between the parentheses is either a full boolean expression
    /// or a comparison triple (`left op right`).
    fn parse_bool_unit_paren(&self, pair: Pair) -> ParseResult<Box<ast::BoolUnit>> {
        let pair_for_error = pair.clone();
        let pos = get_pos(&pair);
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        let filtered: Vec<_> = inner_pairs
            .into_iter()
            .filter(|p| p.as_rule() != Rule::lparen && p.as_rule() != Rule::rparen)
            .collect();

        if filtered.len() == 1 && filtered[0].as_rule() == Rule::bool_expr {
            return Ok(Box::new(ast::BoolUnit {
                pos,
                inner: ast::BoolUnitInner::BoolExpr(self.parse_bool_expr(filtered[0].clone())?),
            }));
        }

        self.parse_comparison_pair_triple(pos, &filtered, "bool_unit_paren", &pair_for_error)
    }

    /// Parses a bare comparison expression (`left op right`) into a
    /// [`ast::BoolUnit`].
    fn parse_bool_comparison(&self, pair: Pair) -> ParseResult<Box<ast::BoolUnit>> {
        let pair_for_error = pair.clone();
        let pos = get_pos(&pair);
        let inner_pairs: Vec<_> = pair.into_inner().collect();
        self.parse_comparison_pair_triple(pos, &inner_pairs, "bool_comparison", &pair_for_error)
    }

    /// Validates that `pairs` has exactly three elements (left, op, right) and
    /// delegates to [`parse_comparison_to_bool_unit`](Self::parse_comparison_to_bool_unit).
    fn parse_comparison_pair_triple(
        &self,
        pos: usize,
        pairs: &[Pair],
        context: &'static str,
        pair_for_error: &Pair<'_>,
    ) -> ParseResult<Box<ast::BoolUnit>> {
        if pairs.len() != 3 {
            return Err(grammar_error(context, pair_for_error));
        }

        self.parse_comparison_to_bool_unit(
            pos,
            pairs[0].clone(),
            pairs[1].clone(),
            pairs[2].clone(),
        )
    }

    /// Assembles a comparison triple `(left_pair, op_pair, right_pair)` into a
    /// [`ast::BoolUnit::ComExpr`] node.
    fn parse_comparison_to_bool_unit(
        &self,
        pos: usize,
        left_pair: Pair,
        op_pair: Pair,
        right_pair: Pair,
    ) -> ParseResult<Box<ast::BoolUnit>> {
        let left = self.parse_expr_unit(left_pair)?;
        let op = self.parse_comp_op(op_pair)?;
        let right = self.parse_expr_unit(right_pair)?;

        Ok(Box::new(ast::BoolUnit {
            pos,
            inner: ast::BoolUnitInner::ComExpr(Box::new(ast::ComExpr { op, left, right })),
        }))
    }

    /// Parses a comparison operator token (`<`, `>`, `<=`, `>=`, `==`, `!=`).
    fn parse_comp_op(&self, pair: Pair) -> ParseResult<ast::ComOp> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::op_lt => return Ok(ast::ComOp::Lt),
                Rule::op_gt => return Ok(ast::ComOp::Gt),
                Rule::op_le => return Ok(ast::ComOp::Le),
                Rule::op_ge => return Ok(ast::ComOp::Ge),
                Rule::op_eq => return Ok(ast::ComOp::Eq),
                Rule::op_ne => return Ok(ast::ComOp::Ne),
                _ => {}
            }
        }
        Err(grammar_error("comp_op", &pair_for_error))
    }

    /// Parses an arithmetic expression, handling `+` and `-` as
    /// left-associative binary operators over arithmetic terms.
    pub(crate) fn parse_arith_expr(&self, pair: Pair) -> ParseResult<Box<ast::ArithExpr>> {
        let pair_for_error = pair.clone();
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        if inner_pairs.is_empty() {
            return Err(grammar_error("arith_expr", &pair_for_error));
        }

        let mut expr = self.parse_arith_term(inner_pairs[0].clone())?;

        let mut i = 1;
        while i < inner_pairs.len() {
            if inner_pairs[i].as_rule() == Rule::arith_add_op {
                let op = self.parse_arith_add_op(inner_pairs[i].clone())?;
                let right = self.parse_arith_term(inner_pairs[i + 1].clone())?;

                expr = Box::new(ast::ArithExpr {
                    pos: expr.pos,
                    inner: ast::ArithExprInner::ArithBiOpExpr(Box::new(ast::ArithBiOpExpr {
                        op,
                        left: expr,
                        right,
                    })),
                });
                i += 2;
            } else {
                i += 1;
            }
        }

        Ok(expr)
    }

    /// Parses an arithmetic term, handling `*` and `/` as left-associative
    /// binary operators over expression units.
    fn parse_arith_term(&self, pair: Pair) -> ParseResult<Box<ast::ArithExpr>> {
        let pair_for_error = pair.clone();
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        if inner_pairs.is_empty() {
            return Err(grammar_error("arith_term", &pair_for_error));
        }

        let first_unit = self.parse_expr_unit(inner_pairs[0].clone())?;
        let mut expr = Box::new(ast::ArithExpr {
            pos: first_unit.pos,
            inner: ast::ArithExprInner::ExprUnit(first_unit),
        });

        let mut i = 1;
        while i < inner_pairs.len() {
            if inner_pairs[i].as_rule() == Rule::arith_mul_op {
                let op = self.parse_arith_mul_op(inner_pairs[i].clone())?;
                let right_unit = self.parse_expr_unit(inner_pairs[i + 1].clone())?;
                let right = Box::new(ast::ArithExpr {
                    pos: right_unit.pos,
                    inner: ast::ArithExprInner::ExprUnit(right_unit),
                });

                expr = Box::new(ast::ArithExpr {
                    pos: expr.pos,
                    inner: ast::ArithExprInner::ArithBiOpExpr(Box::new(ast::ArithBiOpExpr {
                        op,
                        left: expr,
                        right,
                    })),
                });
                i += 2;
            } else {
                i += 1;
            }
        }

        Ok(expr)
    }

    /// Parses an additive operator token (`+` or `-`).
    fn parse_arith_add_op(&self, pair: Pair) -> ParseResult<ast::ArithBiOp> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::op_add => return Ok(ast::ArithBiOp::Add),
                Rule::op_sub => return Ok(ast::ArithBiOp::Sub),
                _ => {}
            }
        }
        Err(grammar_error("arith_add_op", &pair_for_error))
    }

    /// Parses a multiplicative operator token (`*` or `/`).
    fn parse_arith_mul_op(&self, pair: Pair) -> ParseResult<ast::ArithBiOp> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::op_mul => return Ok(ast::ArithBiOp::Mul),
                Rule::op_div => return Ok(ast::ArithBiOp::Div),
                _ => {}
            }
        }
        Err(grammar_error("arith_mul_op", &pair_for_error))
    }

    /// Parses an expression unit – the lowest-precedence leaf of an arithmetic
    /// expression.
    ///
    /// Recognised forms (in order):
    /// 1. Negated integer literal: `-N`
    /// 2. Parenthesised arithmetic expression: `(<arith_expr>)`
    /// 3. Function call: `fn_call`
    /// 4. Integer literal: `N`
    /// 5. Reference: `&id`
    /// 6. Identifier / array-access / member-access chain: `id[…].…`
    pub(crate) fn parse_expr_unit(&self, pair: Pair) -> ParseResult<Box<ast::ExprUnit>> {
        let pair_for_error = pair.clone();
        let pos = get_pos(&pair);
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        let filtered: Vec<_> = inner_pairs
            .iter()
            .filter(|p| !matches!(p.as_rule(), Rule::lparen | Rule::rparen))
            .cloned()
            .collect();

        if filtered.len() == 2
            && filtered[0].as_rule() == Rule::op_sub
            && filtered[1].as_rule() == Rule::num
        {
            let num = parse_num(filtered[1].clone())?;
            return Ok(Box::new(ast::ExprUnit {
                pos,
                inner: ast::ExprUnitInner::Num(-num),
            }));
        }

        if filtered.len() == 1 && filtered[0].as_rule() == Rule::arith_expr {
            return Ok(Box::new(ast::ExprUnit {
                pos,
                inner: ast::ExprUnitInner::ArithExpr(self.parse_arith_expr(filtered[0].clone())?),
            }));
        }

        if !filtered.is_empty() && filtered[0].as_rule() == Rule::fn_call {
            return Ok(Box::new(ast::ExprUnit {
                pos,
                inner: ast::ExprUnitInner::FnCall(self.parse_fn_call(filtered[0].clone())?),
            }));
        }

        if filtered.len() == 1 && filtered[0].as_rule() == Rule::num {
            let num = parse_num(filtered[0].clone())?;
            return Ok(Box::new(ast::ExprUnit {
                pos,
                inner: ast::ExprUnitInner::Num(num),
            }));
        }

        if filtered.len() == 2
            && filtered[0].as_rule() == Rule::ampersand
            && filtered[1].as_rule() == Rule::identifier
        {
            let id = filtered[1].as_str().to_string();
            return Ok(Box::new(ast::ExprUnit {
                pos,
                inner: ast::ExprUnitInner::Reference(id),
            }));
        }

        if !inner_pairs.is_empty() && inner_pairs[0].as_rule() == Rule::identifier {
            let id = inner_pairs[0].as_str().to_string();

            let mut base = Box::new(ast::LeftVal {
                pos,
                inner: ast::LeftValInner::Id(id),
            });

            let mut i = 1;
            while i < inner_pairs.len() {
                match inner_pairs[i].as_rule() {
                    Rule::expr_suffix => {
                        base = self.parse_expr_suffix(base, inner_pairs[i].clone())?;
                        i += 1;
                    }
                    _ => break,
                }
            }

            return left_val_to_expr_unit(*base);
        }

        Err(grammar_error("expr_unit", &pair_for_error))
    }

    /// Parses an array index expression, which is either an integer literal or
    /// an identifier.
    pub(crate) fn parse_index_expr(&self, pair: Pair) -> ParseResult<Box<ast::IndexExpr>> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::num => {
                    let num = parse_num(inner)? as usize;
                    return Ok(Box::new(ast::IndexExpr {
                        inner: ast::IndexExprInner::Num(num),
                    }));
                }
                Rule::identifier => {
                    return Ok(Box::new(ast::IndexExpr {
                        inner: ast::IndexExprInner::Id(inner.as_str().to_string()),
                    }));
                }
                _ => {}
            }
        }
        Err(grammar_error("index_expr", &pair_for_error))
    }

    /// Parses a function call, dispatching to either a module-prefixed call
    /// (`mod::func(...)`) or a local call (`func(...)`).
    pub(crate) fn parse_fn_call(&self, pair: Pair) -> ParseResult<Box<ast::FnCall>> {
        let pair_for_error = pair.clone();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::module_prefixed_call => {
                    return self.parse_module_prefixed_call(inner);
                }
                Rule::local_call => {
                    return self.parse_local_call(inner);
                }
                _ => {}
            }
        }
        Err(grammar_error("fn_call", &pair_for_error))
    }

    /// Parses a module-prefixed function call such as `module::func(args)`.
    ///
    /// All but the last identifier form the module path; the last is the
    /// function name.
    fn parse_module_prefixed_call(&self, pair: Pair) -> ParseResult<Box<ast::FnCall>> {
        let inner_pairs: Vec<_> = pair.into_inner().collect();
        let mut idents: Vec<String> = Vec::new();
        let mut vals = Vec::new();

        for inner in &inner_pairs {
            match inner.as_rule() {
                Rule::identifier => idents.push(inner.as_str().to_string()),
                Rule::right_val_list => vals = self.parse_right_val_list(inner.clone())?,
                _ => {}
            }
        }

        let name = idents.pop().unwrap_or_default();
        let module_prefix = if idents.is_empty() {
            None
        } else {
            Some(idents.join("::"))
        };

        Ok(Box::new(ast::FnCall {
            module_prefix,
            name,
            vals,
        }))
    }

    /// Parses a local (non-module-qualified) function call such as `func(args)`.
    fn parse_local_call(&self, pair: Pair) -> ParseResult<Box<ast::FnCall>> {
        let mut name = String::new();
        let mut vals = Vec::new();

        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::identifier => name = inner.as_str().to_string(),
                Rule::right_val_list => vals = self.parse_right_val_list(inner)?,
                _ => {}
            }
        }

        Ok(Box::new(ast::FnCall {
            module_prefix: None,
            name,
            vals,
        }))
    }

    /// Parses a left value starting from an identifier, then processes any
    /// trailing `expr_suffix` nodes to build up array-access or member-access
    /// chains.
    pub(crate) fn parse_left_val(&self, pair: Pair) -> ParseResult<Box<ast::LeftVal>> {
        let pair_for_error = pair.clone();
        let pos = get_pos(&pair);
        let inner_pairs: Vec<_> = pair.into_inner().collect();

        if inner_pairs.is_empty() {
            return Err(grammar_error("left_val", &pair_for_error));
        }

        let id = inner_pairs[0].as_str().to_string();

        let mut base = Box::new(ast::LeftVal {
            pos,
            inner: ast::LeftValInner::Id(id),
        });

        let mut i = 1;
        while i < inner_pairs.len() {
            match inner_pairs[i].as_rule() {
                Rule::expr_suffix => {
                    base = self.parse_expr_suffix(base, inner_pairs[i].clone())?;
                    i += 1;
                }
                _ => break,
            }
        }

        Ok(base)
    }

    /// Applies a single left-value suffix to `base`.
    ///
    /// - `[index_expr]` → wraps `base` in an [`ast::ArrayExpr`].
    /// - `.identifier`  → wraps `base` in an [`ast::MemberExpr`].
    pub(crate) fn parse_expr_suffix(
        &self,
        base: Box<ast::LeftVal>,
        suffix: Pair,
    ) -> ParseResult<Box<ast::LeftVal>> {
        let pos = base.pos;

        for inner in suffix.into_inner() {
            match inner.as_rule() {
                Rule::lbracket | Rule::rbracket | Rule::dot => continue,
                Rule::index_expr => {
                    let idx = self.parse_index_expr(inner)?;
                    return Ok(Box::new(ast::LeftVal {
                        pos,
                        inner: ast::LeftValInner::ArrayExpr(Box::new(ast::ArrayExpr {
                            arr: base,
                            idx,
                        })),
                    }));
                }
                Rule::identifier => {
                    let member_id = inner.as_str().to_string();
                    return Ok(Box::new(ast::LeftVal {
                        pos,
                        inner: ast::LeftValInner::MemberExpr(Box::new(ast::MemberExpr {
                            struct_id: base,
                            member_id,
                        })),
                    }));
                }
                _ => {}
            }
        }

        Ok(base)
    }
}

/// Converts a [`ast::LeftVal`] into the corresponding [`ast::ExprUnit`] variant:
/// - `Id`         → `ExprUnitInner::Id`
/// - `ArrayExpr`  → `ExprUnitInner::ArrayExpr`
/// - `MemberExpr` → `ExprUnitInner::MemberExpr`
fn left_val_to_expr_unit(lval: ast::LeftVal) -> ParseResult<Box<ast::ExprUnit>> {
    let pos = lval.pos;

    match &lval.inner {
        ast::LeftValInner::Id(id) => Ok(Box::new(ast::ExprUnit {
            pos,
            inner: ast::ExprUnitInner::Id(id.clone()),
        })),
        ast::LeftValInner::ArrayExpr(arr_expr) => Ok(Box::new(ast::ExprUnit {
            pos,
            inner: ast::ExprUnitInner::ArrayExpr(arr_expr.clone()),
        })),
        ast::LeftValInner::MemberExpr(mem_expr) => Ok(Box::new(ast::ExprUnit {
            pos,
            inner: ast::ExprUnitInner::MemberExpr(mem_expr.clone()),
        })),
    }
}
