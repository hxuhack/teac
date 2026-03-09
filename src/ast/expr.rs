use super::ops::*;
use super::types::Pos;

#[derive(Debug, Clone)]
pub struct LeftVal {
    pub pos: Pos,
    pub inner: LeftValInner,
}

#[derive(Debug, Clone)]
pub enum LeftValInner {
    Id(String),
    ArrayExpr(Box<ArrayExpr>),
    MemberExpr(Box<MemberExpr>),
}

#[derive(Debug, Clone)]
pub enum IndexExprInner {
    Num(usize),
    Id(String),
}

#[derive(Debug, Clone)]
pub struct IndexExpr {
    pub inner: IndexExprInner,
}

#[derive(Debug, Clone)]
pub struct ArrayExpr {
    pub arr: Box<LeftVal>,
    pub idx: Box<IndexExpr>,
}

#[derive(Debug, Clone)]
pub struct MemberExpr {
    pub struct_id: Box<LeftVal>,
    pub member_id: String,
}

#[derive(Debug, Clone)]
pub struct ArithBiOpExpr {
    pub op: ArithBiOp,
    pub left: Box<ArithExpr>,
    pub right: Box<ArithExpr>,
}

#[derive(Debug, Clone)]
pub enum ArithExprInner {
    ArithBiOpExpr(Box<ArithBiOpExpr>),
    ExprUnit(Box<ExprUnit>),
}

#[derive(Debug, Clone)]
pub struct ArithExpr {
    pub pos: Pos,
    pub inner: ArithExprInner,
}

#[derive(Debug, Clone)]
pub struct ComExpr {
    pub op: ComOp,
    pub left: Box<ExprUnit>,
    pub right: Box<ExprUnit>,
}

#[derive(Debug, Clone)]
pub struct BoolUOpExpr {
    pub op: BoolUOp,
    pub cond: Box<BoolUnit>,
}

#[derive(Debug, Clone)]
pub struct BoolBiOpExpr {
    pub op: BoolBiOp,
    pub left: Box<BoolExpr>,
    pub right: Box<BoolExpr>,
}

#[derive(Debug, Clone)]
pub enum BoolExprInner {
    BoolBiOpExpr(Box<BoolBiOpExpr>),
    BoolUnit(Box<BoolUnit>),
}

#[derive(Debug, Clone)]
pub struct BoolExpr {
    pub pos: Pos,
    pub inner: BoolExprInner,
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum BoolUnitInner {
    ComExpr(Box<ComExpr>),
    BoolExpr(Box<BoolExpr>),
    BoolUOpExpr(Box<BoolUOpExpr>),
}

#[derive(Debug, Clone)]
pub struct BoolUnit {
    pub pos: Pos,
    pub inner: BoolUnitInner,
}

#[derive(Debug, Clone)]
pub struct FnCall {
    pub module_prefix: Option<String>,
    pub name: String,
    pub vals: RightValList,
}

impl FnCall {
    pub fn qualified_name(&self) -> String {
        if let Some(module) = &self.module_prefix {
            format!("{module}::{}", self.name)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExprUnitInner {
    Num(i32),
    Id(String),
    ArithExpr(Box<ArithExpr>),
    FnCall(Box<FnCall>),
    ArrayExpr(Box<ArrayExpr>),
    MemberExpr(Box<MemberExpr>),
}

#[derive(Debug, Clone)]
pub struct ExprUnit {
    pub pos: Pos,
    pub inner: ExprUnitInner,
}

#[derive(Debug, Clone)]
pub enum RightValInner {
    ArithExpr(Box<ArithExpr>),
    BoolExpr(Box<BoolExpr>),
}

#[derive(Debug, Clone)]
pub struct RightVal {
    pub inner: RightValInner,
}

pub type RightValList = Vec<RightVal>;
