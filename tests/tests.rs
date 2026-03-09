// Integration tests for the TeaLang compiler.
// Supports: Native AArch64 Linux, x86/x86_64 Linux (cross-compile + QEMU), macOS (Docker).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Once;

static INIT: Once = Once::new();

fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

fn is_cross_linux() -> bool {
    cfg!(all(
        target_os = "linux",
        any(target_arch = "x86", target_arch = "x86_64")
    ))
}

fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ensure_cross_tools() {
    if is_macos() {
        if !command_exists("docker") {
            panic!(
                "✗ Docker not found.\n\
                 Please install Docker Desktop for macOS: https://www.docker.com/products/docker-desktop"
            );
        }

        let status = Command::new("docker")
            .arg("info")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !status.map(|s| s.success()).unwrap_or(false) {
            panic!(
                "✗ Docker is not running.\n\
                 Please start Docker Desktop."
            );
        }
    } else if is_cross_linux() {
        if !command_exists("aarch64-linux-gnu-gcc") {
            panic!(
                "✗ aarch64-linux-gnu-gcc not found.\n\
                 Please install: sudo apt install gcc-aarch64-linux-gnu"
            );
        }

        if !command_exists("qemu-aarch64") {
            panic!(
                "✗ qemu-aarch64 not found.\n\
                 Please install: sudo apt install qemu-user"
            );
        }
    }
}

fn get_std_o_path() -> PathBuf {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let std_dir = project_root.join("tests").join("std");
    if is_macos() || is_cross_linux() {
        std_dir.join("std-linux.o")
    } else {
        std_dir.join("std.o")
    }
}

fn compile_std_in_docker(std_dir: &Path, o_path: &Path) {
    let o_name = o_path.file_name().unwrap().to_str().unwrap();

    let status = Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("-v")
        .arg(format!("{}:/work", std_dir.display()))
        .arg("-w")
        .arg("/work")
        .arg("--platform")
        .arg("linux/arm64")
        .arg("gcc:latest")
        .arg("gcc")
        .arg("-c")
        .arg("std.c")
        .arg("-o")
        .arg(o_name)
        .status()
        .expect("Failed to run docker");

    assert!(
        status.success(),
        "✗ Failed to compile std.c in Docker (exit {})",
        status.code().unwrap_or(-1)
    );
}

fn compile_std_cross_linux(std_dir: &Path, o_path: &Path) {
    let status = Command::new("aarch64-linux-gnu-gcc")
        .arg("-c")
        .arg("std.c")
        .arg("-o")
        .arg(o_path)
        .current_dir(std_dir)
        .status()
        .expect("Failed to execute aarch64-linux-gnu-gcc");

    assert!(
        status.success(),
        "✗ aarch64-linux-gnu-gcc failed to build {} (exit {}). Ran in {}",
        o_path.display(),
        status.code().unwrap_or(-1),
        std_dir.display()
    );
}

fn ensure_std() {
    INIT.call_once(|| {
        ensure_cross_tools();

        let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let std_dir = project_root.join("tests").join("std");
        let c_path = std_dir.join("std.c");
        let o_path = get_std_o_path();

        let needs_build = match (fs::metadata(&c_path), fs::metadata(&o_path)) {
            (Ok(c_meta), Ok(o_meta)) => match (c_meta.modified(), o_meta.modified()) {
                (Ok(c_m), Ok(o_m)) => c_m > o_m,
                _ => true,
            },
            (Ok(_), Err(_)) => true,
            _ => {
                panic!("✗ Missing tests/std/std.c at {}", c_path.display());
            }
        };

        if needs_build {
            if is_macos() {
                compile_std_in_docker(&std_dir, &o_path);
            } else if is_cross_linux() {
                compile_std_cross_linux(&std_dir, &o_path);
            } else {
                let status = Command::new("gcc")
                    .arg("-c")
                    .arg("std.c")
                    .arg("-o")
                    .arg(&o_path)
                    .current_dir(&std_dir)
                    .status()
                    .expect("Failed to execute gcc");

                assert!(
                    status.success(),
                    "✗ gcc failed to build {} (exit {}). Ran in {}",
                    o_path.display(),
                    status.code().unwrap_or(-1),
                    std_dir.display()
                );
            }
        }
        assert!(
            o_path.is_file(),
            "✗ std.o not found at {}",
            o_path.display()
        );
    });
}

#[inline(always)]
fn launch(dir: &PathBuf, input_file: &str, output_file: &str) -> Output {
    let tool = Path::new(env!("CARGO_BIN_EXE_teac"));
    Command::new(tool)
        .arg(input_file)
        .arg("--emit")
        .arg("asm")
        .arg("-o")
        .arg(output_file)
        .current_dir(dir)
        .output()
        .expect("Failed to execute teac")
}

fn normalize_for_diff_bb(s: &str) -> String {
    let mut out = Vec::new();
    for line in s.lines() {
        let norm = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if norm.is_empty() {
            continue;
        }
        out.push(norm);
    }
    if out.is_empty() {
        String::new()
    } else {
        out.join("\n") + "\n"
    }
}

fn read_to_string_if_exists(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn run_capture(cmd: &mut Command) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    let output = cmd.output()?;
    let code = output.status.code().unwrap_or(-1);
    Ok((code, output.stdout, output.stderr))
}

fn append_line<P: AsRef<Path>>(path: P, line: &str) {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_ref())
        .unwrap_or_else(|e| panic!("Failed to open {} for append: {e}", path.as_ref().display()));
    writeln!(f, "{line}").expect("Failed to append line");
}

fn link_and_run_in_docker(
    build_dir: &Path,
    asm_name: &str,
    std_o: &Path,
    exe_name: &str,
    input: Option<&Path>,
) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    let std_dir = std_o.parent().unwrap();
    let std_o_name = std_o.file_name().unwrap().to_str().unwrap();

    let link_status = Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("-v")
        .arg(format!("{}:/build", build_dir.display()))
        .arg("-v")
        .arg(format!("{}:/std:ro", std_dir.display()))
        .arg("-w")
        .arg("/build")
        .arg("--platform")
        .arg("linux/arm64")
        .arg("gcc:latest")
        .arg("gcc")
        .arg(asm_name)
        .arg(format!("/std/{std_o_name}"))
        .arg("-o")
        .arg(exe_name)
        .arg("-static")
        .status()?;

    if !link_status.success() {
        return Ok((
            link_status.code().unwrap_or(-1),
            Vec::new(),
            b"Linking failed in Docker".to_vec(),
        ));
    }

    let mut run_cmd = Command::new("docker");
    run_cmd
        .arg("run")
        .arg("--rm")
        .arg("-i")
        .arg("-v")
        .arg(format!("{}:/build:ro", build_dir.display()))
        .arg("-w")
        .arg("/build")
        .arg("--platform")
        .arg("linux/arm64")
        .arg("debian:bookworm-slim")
        .arg(format!("./{exe_name}"));

    if let Some(input_path) = input {
        let mut data = Vec::new();
        File::open(input_path)?.read_to_end(&mut data)?;

        let mut child = run_cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&data)?;
        }

        let output = child.wait_with_output()?;
        Ok((
            output.status.code().unwrap_or(-1),
            output.stdout,
            output.stderr,
        ))
    } else {
        run_capture(
            run_cmd
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
    }
}

fn link_cross_linux(
    build_dir: &Path,
    asm_path: &Path,
    std_o: &Path,
    exe_path: &Path,
) -> io::Result<(i32, Vec<u8>)> {
    let output = Command::new("aarch64-linux-gnu-gcc")
        .arg(asm_path)
        .arg(std_o)
        .arg("-o")
        .arg(exe_path)
        .arg("-static")
        .current_dir(build_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    Ok((output.status.code().unwrap_or(-1), output.stderr))
}

fn run_with_qemu(exe: &Path, input: Option<&Path>) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    if let Some(input_path) = input {
        let mut data = Vec::new();
        File::open(input_path)?.read_to_end(&mut data)?;

        let mut child = Command::new("qemu-aarch64")
            .arg(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&data)?;
        }

        let output = child.wait_with_output()?;
        Ok((
            output.status.code().unwrap_or(-1),
            output.stdout,
            output.stderr,
        ))
    } else {
        run_capture(
            Command::new("qemu-aarch64")
                .arg(exe)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
    }
}

fn link_native(
    build_dir: &Path,
    asm_path: &Path,
    std_o: &Path,
    exe_path: &Path,
) -> io::Result<(i32, Vec<u8>)> {
    let output = Command::new("gcc")
        .arg(asm_path)
        .arg(std_o)
        .arg("-o")
        .arg(exe_path)
        .current_dir(build_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    Ok((output.status.code().unwrap_or(-1), output.stderr))
}

fn run_native(exe: &Path, input: Option<&Path>) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    if let Some(input_path) = input {
        let mut data = Vec::new();
        File::open(input_path)?.read_to_end(&mut data)?;

        let mut child = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&data)?;
        }

        let output = child.wait_with_output()?;
        Ok((
            output.status.code().unwrap_or(-1),
            output.stdout,
            output.stderr,
        ))
    } else {
        run_capture(
            Command::new(exe)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
    }
}

fn test_single(test_name: &str) {
    let base_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let case_dir = base_dir.join(test_name);

    let out_dir = case_dir.join("build");
    fs::create_dir_all(&out_dir).expect("Failed to create output dir");

    let tea = case_dir.join(format!("{test_name}.tea"));
    assert!(
        tea.is_file(),
        "✗ {test_name}: Test file not found at {}",
        tea.display()
    );

    let output_name = format!("{test_name}.s");
    let output_path = out_dir.join(&output_name);
    let output = launch(
        &case_dir,
        &format!("{test_name}.tea"),
        output_path.to_str().unwrap(),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "✗ Compilation failed (exit {}). teac stderr:\n{stderr}",
        output.status.code().unwrap_or(-1)
    );
    assert!(
        stderr.is_empty(),
        "✗ Compilation produced stderr:\n{stderr}"
    );

    assert!(
        output_path.is_file(),
        "Expected compiler to produce {}",
        output_path.display()
    );

    let stdlib = get_std_o_path();
    assert!(
        stdlib.is_file(),
        "✗ std.o not found at {}",
        stdlib.display()
    );

    let input = case_dir.join(format!("{test_name}.in"));
    let expected_out = case_dir.join(format!("{test_name}.out"));
    let actual_out = out_dir.join(format!("{test_name}.out"));

    let input_path = if input.is_file() {
        Some(input.as_path())
    } else {
        None
    };

    let (run_code, run_stdout, run_stderr) = if is_macos() {
        // Use Docker for linking and running on macOS
        link_and_run_in_docker(&out_dir, &output_name, &stdlib, test_name, input_path)
            .expect("Failed to run in Docker")
    } else if is_cross_linux() {
        let exe = out_dir.join(test_name);
        let (link_code, link_err) =
            link_cross_linux(&out_dir, &output_path, &stdlib, &exe).expect("Failed to link");
        assert!(
            link_code == 0,
            "✗ Linking failed (exit {link_code}). Stderr:\n{}",
            String::from_utf8_lossy(&link_err)
        );
        run_with_qemu(&exe, input_path).expect("Failed to run with QEMU")
    } else {
        let exe = out_dir.join(test_name);
        let (link_code, link_err) =
            link_native(&out_dir, &output_path, &stdlib, &exe).expect("Failed to link");
        assert!(
            link_code == 0,
            "✗ Linking failed (exit {link_code}). Stderr:\n{}",
            String::from_utf8_lossy(&link_err)
        );
        run_native(&exe, input_path).expect("Failed to run executable")
    };

    if !run_stderr.is_empty() {
        let stderr_str = String::from_utf8_lossy(&run_stderr);
        if stderr_str.contains("Linking failed") {
            panic!("✗ Linking failed. Stderr:\n{stderr_str}");
        }
    }

    fs::write(&actual_out, &run_stdout)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", actual_out.display()));
    append_line(&actual_out, &run_code.to_string());

    match read_to_string_if_exists(&expected_out).expect("Failed to read expected output file") {
        Some(exp) => {
            let got = fs::read_to_string(&actual_out)
                .unwrap_or_else(|e| panic!("Failed to read {}: {e}", actual_out.display()));
            let exp_norm = normalize_for_diff_bb(&exp);
            let got_norm = normalize_for_diff_bb(&got);
            if exp_norm != got_norm {
                if std::env::var_os("VERBOSE").is_some() {
                    eprintln!("✗ Output mismatch for {test_name}");
                    eprintln!("--- Expected:\n{exp}");
                    eprintln!("--- Got:\n{got}");
                }
                panic!("Output mismatch for {test_name}");
            }
        }
        None => {
            panic!(
                "✗ No expected output file for {test_name} at {}",
                expected_out.display()
            );
        }
    }
}

#[test]
fn dfs() {
    ensure_std();
    test_single("dfs");
}
#[test]
fn bfs() {
    ensure_std();
    test_single("bfs");
}
#[test]
fn big_int_mul() {
    ensure_std();
    test_single("big_int_mul");
}
#[test]
fn bin_search() {
    ensure_std();
    test_single("bin_search");
}
#[test]
fn brainfk() {
    ensure_std();
    test_single("brainfk");
}
#[test]
fn conv() {
    ensure_std();
    test_single("conv");
}
#[test]
fn dijkstra() {
    ensure_std();
    test_single("dijkstra");
}
#[test]
fn expr_eval() {
    ensure_std();
    test_single("expr_eval");
}
#[test]
fn full_conn() {
    ensure_std();
    test_single("full_conn");
}
#[test]
fn hanoi() {
    ensure_std();
    test_single("hanoi");
}
#[test]
fn insert_order() {
    ensure_std();
    test_single("insert_order");
}
#[test]
fn int_io() {
    ensure_std();
    test_single("int_io");
}
#[test]
fn int_split() {
    ensure_std();
    test_single("int_split");
}
#[test]
fn jump_game() {
    ensure_std();
    test_single("jump_game");
}
#[test]
fn line_search() {
    ensure_std();
    test_single("line_search");
}
#[test]
fn long_code() {
    ensure_std();
    test_single("long_code");
}
#[test]
fn long_code2() {
    ensure_std();
    test_single("long_code2");
}
#[test]
fn many_globals() {
    ensure_std();
    test_single("many_globals");
}
#[test]
fn many_locals2() {
    ensure_std();
    test_single("many_locals2");
}
#[test]
fn matrix_mul() {
    ensure_std();
    test_single("matrix_mul");
}
#[test]
fn nested_calls() {
    ensure_std();
    test_single("nested_calls");
}
#[test]
fn nested_loops() {
    ensure_std();
    test_single("nested_loops");
}
#[test]
fn palindrome_number() {
    ensure_std();
    test_single("palindrome_number");
}
#[test]
fn register_alloca() {
    ensure_std();
    test_single("register_alloca");
}
#[test]
fn short_circuit3() {
    ensure_std();
    test_single("short_circuit3");
}
#[test]
fn sort_test5() {
    ensure_std();
    test_single("sort_test5");
}
#[test]
fn sort_test7() {
    ensure_std();
    test_single("sort_test7");
}
#[test]
fn sort() {
    ensure_std();
    test_single("sort");
}
#[test]
fn unique_path() {
    ensure_std();
    test_single("unique_path");
}

#[test]
fn type_infer() {
    ensure_std();
    test_single("type_infer");
}
