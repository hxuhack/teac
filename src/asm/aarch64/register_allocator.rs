use std::collections::{HashMap, HashSet, VecDeque};

use super::inst::Inst;
use super::types::{Addr, IndexOperand, Operand, RegSize, Register};
use crate::asm::common::StackFrame;
use crate::asm::error::Error;
use crate::common::graph::{BackwardLiveness, Graph, VregSet};

const NUM_COLORS: usize = 8;
const ALLOCATABLE_REGS: [u8; NUM_COLORS] = [8, 9, 10, 11, 12, 13, 14, 15];

const SCRATCH0: u8 = 16;
const SCRATCH1: u8 = 17;

#[derive(Debug, Clone)]
pub struct AllocationResult {
    pub coloring: HashMap<usize, u8>,
    pub spilled: Vec<usize>,
}

pub fn allocate(instructions: &[Inst]) -> AllocationResult {
    if instructions.is_empty() {
        return AllocationResult::empty();
    }

    let cfg = Graph::from_nodes(instructions);
    let gen: Vec<VregSet> = instructions
        .iter()
        .map(|i| VregSet(i.used_vregs()))
        .collect();
    let kill: Vec<VregSet> = instructions
        .iter()
        .map(|i| VregSet(i.defined_vregs()))
        .collect();
    let liveness = BackwardLiveness::<VregSet>::compute(&gen, &kill, &cfg);
    let mut graph = InterferenceGraph::build(instructions, &liveness);
    graph.color()
}

pub fn rewrite_insts(
    insts: &[Inst],
    alloc: &AllocationResult,
    frame: &StackFrame,
) -> Result<Vec<Inst>, Error> {
    let mut rewriter = InstRewriter::new(alloc, frame);
    rewriter.rewrite_all(insts)?;
    rewriter.verify_no_vregs()?;
    Ok(rewriter.into_output())
}

struct InterferenceGraph {
    nodes: HashSet<usize>,
    adjacency: HashMap<usize, HashSet<usize>>,
}

impl InterferenceGraph {
    fn build(instructions: &[Inst], liveness: &BackwardLiveness<VregSet>) -> Self {
        let nodes: HashSet<usize> = instructions
            .iter()
            .flat_map(|inst| inst.used_vregs().into_iter().chain(inst.defined_vregs()))
            .collect();

        if nodes.is_empty() {
            return Self {
                nodes,
                adjacency: HashMap::new(),
            };
        }

        let mut adjacency: HashMap<usize, HashSet<usize>> =
            nodes.iter().map(|&v| (v, HashSet::new())).collect();

        for (i, inst) in instructions.iter().enumerate() {
            for d in inst.defined_vregs() {
                for &r in &liveness.live_out[i].0 {
                    if d != r {
                        adjacency.entry(d).or_default().insert(r);
                        adjacency.entry(r).or_default().insert(d);
                    }
                }
            }
        }

        Self { nodes, adjacency }
    }

    fn degree(&self, v: usize) -> usize {
        self.adjacency.get(&v).map(|s| s.len()).unwrap_or(0)
    }

    fn color(&mut self) -> AllocationResult {
        if self.nodes.is_empty() {
            return AllocationResult::empty();
        }

        let (stack, potential_spills) = self.simplify();
        self.select(stack, potential_spills)
    }

    fn simplify(&mut self) -> (Vec<usize>, HashSet<usize>) {
        let mut degree: HashMap<usize, usize> =
            self.nodes.iter().map(|&v| (v, self.degree(v))).collect();

        let mut low_degree: VecDeque<usize> = self
            .nodes
            .iter()
            .copied()
            .filter(|v| degree[v] < NUM_COLORS)
            .collect();
        let mut in_low: HashSet<usize> = low_degree.iter().copied().collect();

        let mut removed: HashSet<usize> = HashSet::new();
        let mut stack: Vec<usize> = Vec::with_capacity(self.nodes.len());
        let mut potential_spills: HashSet<usize> = HashSet::new();

        while stack.len() < self.nodes.len() {
            let pick = self.pick_node(
                &mut low_degree,
                &mut in_low,
                &removed,
                &degree,
                &mut potential_spills,
            );

            removed.insert(pick);
            stack.push(pick);

            if let Some(neighbors) = self.adjacency.get(&pick) {
                for &u in neighbors {
                    if removed.contains(&u) {
                        continue;
                    }
                    if let Some(d) = degree.get_mut(&u) {
                        if *d > 0 {
                            *d -= 1;
                            if *d < NUM_COLORS && !in_low.contains(&u) {
                                low_degree.push_back(u);
                                in_low.insert(u);
                            }
                        }
                    }
                }
            }
        }

        (stack, potential_spills)
    }

    fn pick_node(
        &self,
        low_degree: &mut VecDeque<usize>,
        in_low: &mut HashSet<usize>,
        removed: &HashSet<usize>,
        degree: &HashMap<usize, usize>,
        potential_spills: &mut HashSet<usize>,
    ) -> usize {
        while let Some(v) = low_degree.pop_front() {
            in_low.remove(&v);
            if !removed.contains(&v) {
                return v;
            }
        }

        let v = self
            .nodes
            .iter()
            .filter(|v| !removed.contains(v))
            .max_by_key(|v| degree.get(v).copied().unwrap_or(0))
            .copied()
            .expect("graph should not be empty");

        potential_spills.insert(v);
        v
    }

    fn select(&self, mut stack: Vec<usize>, potential_spills: HashSet<usize>) -> AllocationResult {
        let mut coloring: HashMap<usize, u8> = HashMap::new();
        let mut spilled: Vec<usize> = Vec::new();

        while let Some(v) = stack.pop() {
            let used_colors: HashSet<u8> = self
                .adjacency
                .get(&v)
                .into_iter()
                .flatten()
                .filter_map(|u| coloring.get(u).copied())
                .collect();

            if let Some(&color) = ALLOCATABLE_REGS.iter().find(|c| !used_colors.contains(c)) {
                coloring.insert(v, color);
            } else {
                spilled.push(v);
            }
        }

        spilled.sort_by_key(|v| (!potential_spills.contains(v), *v));

        AllocationResult { coloring, spilled }
    }
}

impl AllocationResult {
    fn empty() -> Self {
        Self {
            coloring: HashMap::new(),
            spilled: Vec::new(),
        }
    }
}

struct InstRewriter<'a> {
    alloc: &'a AllocationResult,
    frame: &'a StackFrame,
    output: Vec<Inst>,
}

impl<'a> InstRewriter<'a> {
    fn new(alloc: &'a AllocationResult, frame: &'a StackFrame) -> Self {
        Self {
            alloc,
            frame,
            output: Vec::new(),
        }
    }

    fn into_output(self) -> Vec<Inst> {
        self.output
    }

    fn rewrite_all(&mut self, insts: &[Inst]) -> Result<(), Error> {
        for inst in insts {
            self.rewrite_inst(inst)?;
        }
        Ok(())
    }

    fn verify_no_vregs(&self) -> Result<(), Error> {
        for inst in &self.output {
            if !inst.used_vregs().is_empty() || !inst.defined_vregs().is_empty() {
                return Err(Error::Internal(format!(
                    "rewrite left virtual regs behind: {inst:?}"
                )));
            }
        }
        Ok(())
    }

    fn color_of(&self, v: usize) -> Option<u8> {
        self.alloc.coloring.get(&v).copied()
    }

    fn is_spilled(&self, v: usize) -> bool {
        self.frame.spill_slot(v).is_some()
    }

    fn map_reg(&self, r: Register) -> MappedReg {
        match r {
            Register::Virtual(v) => match self.color_of(v) {
                Some(p) => MappedReg::Colored {
                    reg: Register::Physical(p),
                    vreg: v,
                },
                None => MappedReg::Spilled { vreg: v },
            },
            other => MappedReg::Physical { reg: other },
        }
    }

    fn emit_spill_load(&mut self, v: usize, size: RegSize, into: u8) -> Result<(), Error> {
        let slot = self
            .frame
            .spill_slot(v)
            .ok_or_else(|| Error::Internal(format!("missing spill slot for vreg {v}")))?;

        self.output.push(Inst::Ldr {
            size,
            dst: Register::Physical(into),
            addr: Addr::BaseOff {
                base: Register::Physical(29),
                offset: slot.offset_from_fp,
            },
        });
        Ok(())
    }

    fn emit_spill_store(&mut self, v: usize, size: RegSize, from: u8) -> Result<(), Error> {
        let slot = self
            .frame
            .spill_slot(v)
            .ok_or_else(|| Error::Internal(format!("missing spill slot for vreg {v}")))?;

        self.output.push(Inst::Str {
            size,
            src: Register::Physical(from),
            addr: Addr::BaseOff {
                base: Register::Physical(29),
                offset: slot.offset_from_fp,
            },
        });
        Ok(())
    }

    fn load_src_reg(&mut self, r: Register, size: RegSize, scratch: u8) -> Result<Register, Error> {
        match self.map_reg(r) {
            MappedReg::Physical { reg } => Ok(reg),
            MappedReg::Colored { reg, vreg } => {
                if self.is_spilled(vreg) {
                    self.emit_spill_load(vreg, size, scratch)?;
                    Ok(Register::Physical(scratch))
                } else {
                    Ok(reg)
                }
            }
            MappedReg::Spilled { vreg } => {
                self.emit_spill_load(vreg, size, scratch)?;
                Ok(Register::Physical(scratch))
            }
        }
    }

    fn load_src_operand(
        &mut self,
        op: Operand,
        size: RegSize,
        scratch: u8,
    ) -> Result<Operand, Error> {
        match op {
            Operand::Immediate(i) => Ok(Operand::Immediate(i)),
            Operand::Register(r) => Ok(Operand::Register(self.load_src_reg(r, size, scratch)?)),
        }
    }

    fn rewrite_inst(&mut self, inst: &Inst) -> Result<(), Error> {
        match inst {
            Inst::Label(name) => self.output.push(Inst::Label(name.clone())),
            Inst::Mov { size, dst, src } => self.rewrite_mov(*size, *dst, *src)?,
            Inst::BinOp {
                op,
                size,
                dst,
                lhs,
                rhs,
            } => self.rewrite_binop(*op, *size, *dst, *lhs, *rhs)?,
            Inst::Cmp { size, lhs, rhs } => self.rewrite_cmp(*size, *lhs, *rhs)?,
            Inst::Ldr { size, dst, addr } => self.rewrite_ldr(*size, *dst, addr)?,
            Inst::Str { size, src, addr } => self.rewrite_str(*size, *src, addr)?,
            Inst::Lea { dst, addr } => self.rewrite_lea(*dst, addr)?,
            Inst::Gep {
                dst,
                base,
                index,
                scale,
            } => self.rewrite_gep(*dst, *base, *index, *scale)?,
            // Pass-through instructions.
            Inst::B { label } => self.output.push(Inst::B {
                label: label.clone(),
            }),
            Inst::BCond { cond, label } => self.output.push(Inst::BCond {
                cond: *cond,
                label: label.clone(),
            }),
            Inst::Bl { func } => self.output.push(Inst::Bl { func: func.clone() }),
            Inst::SaveCallerRegs => self.output.push(Inst::SaveCallerRegs),
            Inst::RestoreCallerRegs => self.output.push(Inst::RestoreCallerRegs),
            Inst::SubSp { imm } => self.output.push(Inst::SubSp { imm: *imm }),
            Inst::AddSp { imm } => self.output.push(Inst::AddSp { imm: *imm }),
            Inst::Ret => self.output.push(Inst::Ret),
        }
        Ok(())
    }

    fn rewrite_mov(&mut self, size: RegSize, dst: Register, src: Operand) -> Result<(), Error> {
        let src_op = self.load_src_operand(src, size, SCRATCH1)?;

        match self.map_reg(dst) {
            MappedReg::Physical { reg } => {
                self.output.push(Inst::Mov {
                    size,
                    dst: reg,
                    src: src_op,
                });
            }
            MappedReg::Colored { reg, vreg } => {
                if self.is_spilled(vreg) {
                    let from = self.operand_to_phys_reg(src_op, size, SCRATCH0)?;
                    self.emit_spill_store(vreg, size, from)?;
                } else {
                    self.output.push(Inst::Mov {
                        size,
                        dst: reg,
                        src: src_op,
                    });
                }
            }
            MappedReg::Spilled { vreg } => {
                let from = self.operand_to_phys_reg(src_op, size, SCRATCH0)?;
                self.emit_spill_store(vreg, size, from)?;
            }
        }
        Ok(())
    }

    fn rewrite_binop(
        &mut self,
        op: crate::asm::aarch64::BinOp,
        size: RegSize,
        dst: Register,
        lhs: Register,
        rhs: Operand,
    ) -> Result<(), Error> {
        let lhs_reg = self.load_src_reg(lhs, size, SCRATCH0)?;
        let rhs_op = self.load_src_operand(rhs, size, SCRATCH1)?;

        match self.map_reg(dst) {
            MappedReg::Physical { reg } | MappedReg::Colored { reg, .. } => {
                let (final_dst, vreg) = match self.map_reg(dst) {
                    MappedReg::Colored { reg: _, vreg } if self.is_spilled(vreg) => {
                        (Register::Physical(SCRATCH0), Some(vreg))
                    }
                    _ => (reg, None),
                };

                self.output.push(Inst::BinOp {
                    op,
                    size,
                    dst: final_dst,
                    lhs: lhs_reg,
                    rhs: rhs_op,
                });

                if let Some(v) = vreg {
                    self.emit_spill_store(v, size, SCRATCH0)?;
                }
            }
            MappedReg::Spilled { vreg } => {
                self.output.push(Inst::BinOp {
                    op,
                    size,
                    dst: Register::Physical(SCRATCH0),
                    lhs: lhs_reg,
                    rhs: rhs_op,
                });
                self.emit_spill_store(vreg, size, SCRATCH0)?;
            }
        }
        Ok(())
    }

    fn rewrite_cmp(&mut self, size: RegSize, lhs: Register, rhs: Operand) -> Result<(), Error> {
        let lhs_reg = self.load_src_reg(lhs, size, SCRATCH0)?;
        let rhs_op = self.load_src_operand(rhs, size, SCRATCH1)?;

        self.output.push(Inst::Cmp {
            size,
            lhs: lhs_reg,
            rhs: rhs_op,
        });
        Ok(())
    }

    fn rewrite_ldr(&mut self, size: RegSize, dst: Register, addr: &Addr) -> Result<(), Error> {
        let (addr_rewritten, base_used_scratch) = self.rewrite_addr(addr, SCRATCH0)?;

        let scratch_for_dst = if base_used_scratch {
            SCRATCH1
        } else {
            SCRATCH0
        };

        match self.map_reg(dst) {
            MappedReg::Physical { reg } => {
                self.output.push(Inst::Ldr {
                    size,
                    dst: reg,
                    addr: addr_rewritten,
                });
            }
            MappedReg::Colored { reg, vreg } => {
                if self.is_spilled(vreg) {
                    self.output.push(Inst::Ldr {
                        size,
                        dst: Register::Physical(scratch_for_dst),
                        addr: addr_rewritten,
                    });
                    self.emit_spill_store(vreg, size, scratch_for_dst)?;
                } else {
                    self.output.push(Inst::Ldr {
                        size,
                        dst: reg,
                        addr: addr_rewritten,
                    });
                }
            }
            MappedReg::Spilled { vreg } => {
                self.output.push(Inst::Ldr {
                    size,
                    dst: Register::Physical(scratch_for_dst),
                    addr: addr_rewritten,
                });
                self.emit_spill_store(vreg, size, scratch_for_dst)?;
            }
        }
        Ok(())
    }

    fn rewrite_str(&mut self, size: RegSize, src: Register, addr: &Addr) -> Result<(), Error> {
        let (addr_rewritten, base_used_scratch) = self.rewrite_addr(addr, SCRATCH0)?;

        let scratch_for_src = if base_used_scratch {
            SCRATCH1
        } else {
            SCRATCH0
        };
        let src_reg = self.load_src_reg(src, size, scratch_for_src)?;

        self.output.push(Inst::Str {
            size,
            src: src_reg,
            addr: addr_rewritten,
        });
        Ok(())
    }

    fn rewrite_lea(&mut self, dst: Register, addr: &Addr) -> Result<(), Error> {
        let (addr_rewritten, base_used_scratch) = self.rewrite_addr(addr, SCRATCH0)?;

        let scratch_for_dst = if base_used_scratch {
            SCRATCH1
        } else {
            SCRATCH0
        };

        match self.map_reg(dst) {
            MappedReg::Physical { reg } => {
                self.output.push(Inst::Lea {
                    dst: reg,
                    addr: addr_rewritten,
                });
            }
            MappedReg::Colored { reg, vreg } => {
                if self.is_spilled(vreg) {
                    self.output.push(Inst::Lea {
                        dst: Register::Physical(scratch_for_dst),
                        addr: addr_rewritten,
                    });
                    self.emit_spill_store(vreg, RegSize::X64, scratch_for_dst)?;
                } else {
                    self.output.push(Inst::Lea {
                        dst: reg,
                        addr: addr_rewritten,
                    });
                }
            }
            MappedReg::Spilled { vreg } => {
                self.output.push(Inst::Lea {
                    dst: Register::Physical(scratch_for_dst),
                    addr: addr_rewritten,
                });
                self.emit_spill_store(vreg, RegSize::X64, scratch_for_dst)?;
            }
        }
        Ok(())
    }

    fn rewrite_gep(
        &mut self,
        dst: Register,
        base: Register,
        index: IndexOperand,
        scale: i64,
    ) -> Result<(), Error> {
        let base_reg = self.load_src_reg(base, RegSize::X64, SCRATCH0)?;
        let base_used_scratch = matches!(base_reg, Register::Physical(r) if r == SCRATCH0);

        let index_scratch = if base_used_scratch {
            SCRATCH1
        } else {
            SCRATCH0
        };
        let index_rewritten = match index {
            IndexOperand::Imm(i) => IndexOperand::Imm(i),
            IndexOperand::Reg(r) => {
                IndexOperand::Reg(self.load_src_reg(r, RegSize::W32, index_scratch)?)
            }
        };

        let dst_scratch = if base_used_scratch {
            SCRATCH1
        } else {
            SCRATCH0
        };

        match self.map_reg(dst) {
            MappedReg::Physical { reg } => {
                self.output.push(Inst::Gep {
                    dst: reg,
                    base: base_reg,
                    index: index_rewritten,
                    scale,
                });
            }
            MappedReg::Colored { reg, vreg } => {
                if self.is_spilled(vreg) {
                    self.output.push(Inst::Gep {
                        dst: Register::Physical(dst_scratch),
                        base: base_reg,
                        index: index_rewritten,
                        scale,
                    });
                    self.emit_spill_store(vreg, RegSize::X64, dst_scratch)?;
                } else {
                    self.output.push(Inst::Gep {
                        dst: reg,
                        base: base_reg,
                        index: index_rewritten,
                        scale,
                    });
                }
            }
            MappedReg::Spilled { vreg } => {
                self.output.push(Inst::Gep {
                    dst: Register::Physical(dst_scratch),
                    base: base_reg,
                    index: index_rewritten,
                    scale,
                });
                self.emit_spill_store(vreg, RegSize::X64, dst_scratch)?;
            }
        }
        Ok(())
    }

    fn rewrite_addr(&mut self, addr: &Addr, scratch: u8) -> Result<(Addr, bool), Error> {
        match addr {
            Addr::Global(sym) => Ok((Addr::Global(sym.clone()), false)),
            Addr::BaseOff { base, offset } => match base {
                Register::Virtual(v) => {
                    if let Some(p) = self.color_of(*v) {
                        Ok((
                            Addr::BaseOff {
                                base: Register::Physical(p),
                                offset: *offset,
                            },
                            false,
                        ))
                    } else if self.is_spilled(*v) {
                        self.emit_spill_load(*v, RegSize::X64, scratch)?;
                        Ok((
                            Addr::BaseOff {
                                base: Register::Physical(scratch),
                                offset: *offset,
                            },
                            true,
                        ))
                    } else {
                        Ok((
                            Addr::BaseOff {
                                base: Register::Physical(scratch),
                                offset: *offset,
                            },
                            true,
                        ))
                    }
                }
                other => Ok((
                    Addr::BaseOff {
                        base: *other,
                        offset: *offset,
                    },
                    false,
                )),
            },
        }
    }

    fn operand_to_phys_reg(
        &mut self,
        op: Operand,
        size: RegSize,
        scratch: u8,
    ) -> Result<u8, Error> {
        match op {
            Operand::Immediate(imm) => {
                self.output.push(Inst::Mov {
                    size,
                    dst: Register::Physical(scratch),
                    src: Operand::Immediate(imm),
                });
                Ok(scratch)
            }
            Operand::Register(Register::Physical(n)) => Ok(n),
            Operand::Register(Register::StackPointer) => {
                Err(Error::Internal("cannot use SP as source".into()))
            }
            Operand::Register(Register::Virtual(_)) => {
                Err(Error::Internal("unexpected vreg in operand".into()))
            }
        }
    }
}

enum MappedReg {
    Physical { reg: Register },
    Colored { reg: Register, vreg: usize },
    Spilled { vreg: usize },
}
