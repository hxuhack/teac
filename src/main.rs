mod asm;
mod ast;
mod common;
mod ir;
mod opt;
mod parser;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use common::Generator;
use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::Path,
};

#[derive(Copy, Clone, Debug, PartialEq, ValueEnum)]
enum EmitTarget {
    Ast,
    Ir,
    Asm,
}

#[derive(Parser, Debug)]
#[command(name = "teac")]
#[command(about = "A compiler written in Rust for TeaLang")]
struct Cli {
    #[clap(value_name = "FILE")]
    input: String,

    #[arg(long, value_enum, ignore_case = true, default_value = "asm")]
    emit: EmitTarget,

    #[clap(short, long, value_name = "FILE")]
    output: Option<String>,
}

fn open_writer(output: &Option<String>) -> Result<Box<dyn Write>> {
    let Some(path) = output else {
        return Ok(Box::new(BufWriter::new(io::stdout())));
    };
    let out_path = Path::new(path);
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
    }
    let file = File::create(out_path)
        .with_context(|| format!("failed to create file '{}'", out_path.display()))?;
    Ok(Box::new(BufWriter::new(file)))
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let source = fs::read_to_string(&cli.input)
        .with_context(|| format!("failed to read '{}'", cli.input))?;
    let mut writer = open_writer(&cli.output)?;

    let mut parser = parser::Parser::new(&source);
    parser.generate().context("parse failed")?;

    if cli.emit == EmitTarget::Ast {
        return parser
            .output(&mut writer)
            .context("failed to write AST output");
    }

    let ast = parser
        .program
        .as_ref()
        .context("internal parser state missing AST after parse")?;
    let mut ir_gen = ir::IrGenerator::new(ast);
    ir_gen.generate().context("failed to generate IR")?;

    let pass_manager = opt::FunctionPassManager::with_default_pipeline();
    for func in ir_gen.module.function_list.values_mut() {
        pass_manager.run(func);
    }

    if cli.emit == EmitTarget::Ir {
        return ir_gen
            .output(&mut writer)
            .context("failed to write IR output");
    }

    let mut asm_gen = asm::AArch64AsmGenerator::new(&ir_gen.module, &ir_gen.registry);
    asm_gen.generate().context("assembly generation failed")?;
    asm_gen
        .output(&mut writer)
        .context("failed to write assembly output")
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
