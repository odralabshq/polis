//! Structural tests for architectural boundary enforcement.
//!
//! These tests scan source files to verify that the Clean Architecture
//! boundaries are maintained as the refactor progresses.

use std::path::Path;

/// Collect all `.rs` files under a directory recursively.
fn collect_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_rs_files(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files
}

/// Read a file and strip comment lines to avoid false positives.
fn read_non_comment_lines(path: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            !trimmed.starts_with("//") && !trimmed.starts_with("/*") && !trimmed.starts_with('*')
        })
        .map(String::from)
        .collect()
}

// ── Property 10: No inline JSON branching ─────────────────────────────────────

#[test]
fn no_inline_json_branching_in_commands() {
    let commands_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&commands_dir) {
        let lines = read_non_comment_lines(&file);
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        for (i, line) in lines.iter().enumerate() {
            let lineno = i + 1;
            if line.contains("json: bool") {
                violations.push(format!(
                    "{rel}:{lineno}: found `json: bool` parameter: {line}"
                ));
            }
            let trimmed = line.trim();
            if trimmed.starts_with("if json") || trimmed.starts_with("if !json") {
                violations.push(format!("{rel}:{lineno}: found inline JSON branch: {line}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found inline JSON branching in commands/ — use app.renderer() instead:\n{}",
        violations.join("\n")
    );
}

// ── Property 7: All VM operations route through provisioner ──────────────────

#[test]
fn no_tokio_command_runner_new_outside_infra() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&src_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .to_string_lossy()
            .to_string();
        let rel_normalized = rel.replace('\\', "/");
        if rel_normalized.contains("/infra/") || rel_normalized.ends_with("provisioner.rs") {
            continue;
        }

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            if line.contains("TokioCommandRunner::new") {
                violations.push(format!(
                    "{rel}:{}: TokioCommandRunner::new outside infra/: {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found TokioCommandRunner::new outside infra/ — all VM ops must go through provisioner traits:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_multipass_module_imports() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&src_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            if line.contains("crate::multipass::") || line.contains("use crate::multipass") {
                violations.push(format!("{rel}:{}: found multipass:: import: {line}", i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found multipass:: imports — the multipass module has been deleted:\n{}",
        violations.join("\n")
    );
}

// ── Property 5: Trait bounds over concrete types ──────────────────────────────

#[test]
fn no_concrete_provisioner_types_in_service_signatures() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let infra_dir = src_dir.join("infra");
    let services_dir = src_dir.join("application").join("services");

    let concrete_types = [
        "MultipassProvisioner<",
        "TokioCommandRunner>",
        "StateManager",
    ];

    let mut violations: Vec<String> = Vec::new();

    for dir in [&infra_dir, &services_dir] {
        for file in collect_rs_files(dir) {
            let rel = file
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&file)
                .display()
                .to_string();

            let lines = read_non_comment_lines(&file);
            for (i, line) in lines.iter().enumerate() {
                if !line.contains("fn ") {
                    continue;
                }
                for concrete in &concrete_types {
                    if line.contains(concrete) {
                        violations.push(format!(
                            "{rel}:{}: concrete type `{concrete}` in function signature: {line}",
                            i + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found concrete provisioner/runner types in service/infra function signatures — use trait bounds instead:\n{}",
        violations.join("\n")
    );
}

#[test]
fn infra_has_no_imports_from_commands_or_output() {
    let infra_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("infra");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&infra_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            if line.contains("crate::commands") || line.contains("crate::output") {
                violations.push(format!(
                    "{rel}:{}: forbidden import in infra/: {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "infra/ must not import from commands/ or output/:\n{}",
        violations.join("\n")
    );
}

#[test]
fn infra_has_no_print_macros_outside_tests() {
    let infra_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("infra");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&infra_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        let mut in_test_block = false;
        let mut brace_depth: i32 = 0;
        let mut test_block_start_depth: i32 = 0;

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.contains("#[cfg(test)]") {
                in_test_block = true;
                test_block_start_depth = brace_depth;
            }
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if in_test_block && brace_depth <= test_block_start_depth {
                            in_test_block = false;
                        }
                    }
                    _ => {}
                }
            }
            if in_test_block || trimmed.starts_with("//") {
                continue;
            }
            if line.contains("println!") || line.contains("eprintln!") {
                violations.push(format!(
                    "{rel}:{}: print macro in infra/ outside #[cfg(test)]: {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "infra/ must not use println!/eprintln! outside #[cfg(test)]:\n{}",
        violations.join("\n")
    );
}

// ── Property 4: No duplicate type definitions ─────────────────────────────────

#[test]
fn no_duplicate_agent_type_definitions_in_cli() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let duplicate_names = [
        "FullAgentManifest",
        "FullAgentManifestMetadata",
        "AgentSpec",
        "RuntimeSpec",
        "HealthSpec",
        "SecuritySpec",
        "PortSpec",
        "ResourceSpec",
        "PersistenceSpec",
        "RequirementsSpec",
    ];

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&src_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            for name in &duplicate_names {
                if line.contains(&format!("struct {name}")) && line.contains("struct ") {
                    violations.push(format!(
                        "{rel}:{}: duplicate struct `{name}` found in CLI src: {line}",
                        i + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found duplicate agent type definitions in CLI — use polis_common::agent::* instead:\n{}",
        violations.join("\n")
    );
}

// ── Property 8: Command handlers accept unified AppContext ────────────────────

/// Command files that use `AppContext` fields (output, provisioner, state) must
/// receive `&AppContext` rather than individual loose parameters.
///
/// Exception: thin pass-through handlers that only need a single port trait
/// (e.g. `exec.rs` which only needs `ShellExecutor`) are allowed to take the
/// port directly — they don't need the full AppContext.
#[test]
fn command_handlers_accept_app_context() {
    let commands_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&commands_dir) {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        if !content.contains("pub async fn run(") {
            continue;
        }

        // If the file uses AppContext fields directly (output, provisioner, state_mgr)
        // it must receive &AppContext. Files that only use a single port trait are exempt.
        let uses_app_fields = content.contains("app.output")
            || content.contains("app.provisioner")
            || content.contains("app.state_mgr")
            || content.contains("&app.output")
            || content.contains("&app.provisioner");

        if !uses_app_fields {
            // This file only uses injected port traits — AppContext not required.
            continue;
        }

        let has_app_context = content.contains("app: &AppContext")
            || content.contains("app: &crate::app::AppContext");

        if !has_app_context {
            let rel = file
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&file)
                .display()
                .to_string();
            violations.push(format!(
                "{rel}: uses AppContext fields but pub async fn run() does not accept &AppContext"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "Command handlers that use AppContext fields must accept &AppContext:\n{}",
        violations.join("\n")
    );
}

// ── Property 11: Command handler size limits ──────────────────────────────────

/// Each file in `commands/` must be ≤100 lines of non-test code.
#[test]
fn command_handlers_are_reasonably_sized() {
    let commands_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&commands_dir) {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        let mut in_test_block = false;
        let mut brace_depth: i32 = 0;
        let mut test_block_start_depth: i32 = 0;
        let mut line_count = 0;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains("#[cfg(test)]") {
                in_test_block = true;
                test_block_start_depth = brace_depth;
            }
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if in_test_block && brace_depth <= test_block_start_depth {
                            in_test_block = false;
                        }
                    }
                    _ => {}
                }
            }
            if in_test_block {
                continue;
            }
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                line_count += 1;
            }
        }

        if line_count > 100 {
            let rel = file
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&file)
                .display()
                .to_string();
            violations.push(format!("{rel}: {line_count} non-test lines (limit: 100)"));
        }
    }

    assert!(
        violations.is_empty(),
        "Command handler files exceed 100-line limit — extract logic to application services:\n{}",
        violations.join("\n")
    );
}

// ── Property 13: Standardized confirmation mechanism ─────────────────────────

/// All confirmation prompts in `commands/` must go through `app.confirm()`.
#[test]
fn commands_use_standardized_confirmation() {
    let commands_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&commands_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            if line.contains("std::io::stdin().lock()") || line.contains("io::stdin().lock()") {
                violations.push(format!(
                    "{rel}:{}: direct stdin lock — use app.confirm() instead: {line}",
                    i + 1
                ));
            }
            if line.contains("dialoguer::Confirm::new()") || line.contains("Confirm::new()") {
                violations.push(format!(
                    "{rel}:{}: direct dialoguer::Confirm — use app.confirm() instead: {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Commands must use app.confirm() for user prompts:\n{}",
        violations.join("\n")
    );
}

// ── Property 14: Blocking I/O safety ─────────────────────────────────────────

#[test]
fn infra_async_functions_do_not_use_blocking_fs() {
    let infra_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("infra");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&infra_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        let mut in_async_fn = false;
        let mut in_spawn_blocking = false;
        let mut brace_depth: i32 = 0;
        let mut async_fn_start_depth: i32 = 0;
        let mut spawn_blocking_start_depth: i32 = 0;

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if (trimmed.contains("async fn ") || trimmed.contains("async fn\t"))
                && !trimmed.starts_with("//")
            {
                in_async_fn = true;
                async_fn_start_depth = brace_depth;
            }
            if in_async_fn && line.contains("spawn_blocking") {
                in_spawn_blocking = true;
                spawn_blocking_start_depth = brace_depth;
            }
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if in_spawn_blocking && brace_depth <= spawn_blocking_start_depth {
                            in_spawn_blocking = false;
                        }
                        if in_async_fn && brace_depth <= async_fn_start_depth {
                            in_async_fn = false;
                        }
                    }
                    _ => {}
                }
            }
            if !in_async_fn || in_spawn_blocking || trimmed.starts_with("//") {
                continue;
            }
            if line.contains("std::fs::") || (line.contains("fs::") && line.contains("std::fs")) {
                violations.push(format!(
                    "{rel}:{}: std::fs in async fn outside spawn_blocking — use spawn_blocking instead: {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "infra/ async functions must not use blocking std::fs outside spawn_blocking:\n{}",
        violations.join("\n")
    );
}

// ── New: Application layer boundary checks ────────────────────────────────────

/// application/ must not import from crate::workspace (module deleted).
#[test]
fn application_has_no_workspace_imports() {
    let app_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("application");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&app_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            if line.contains("crate::workspace::") {
                violations.push(format!(
                    "{rel}:{}: forbidden crate::workspace:: import (module deleted): {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "application/ must not import from crate::workspace (module deleted):\n{}",
        violations.join("\n")
    );
}

/// application/ must not use std::fs, std::process::Command, or std::net directly
/// in async functions outside spawn_blocking.
///
/// Exceptions:
/// - std::fs inside spawn_blocking closures is allowed (correct async pattern)
/// - std::fs inside #[cfg(unix)] blocks is allowed (temp file permissions)
/// - std::fs inside #[cfg(test)] blocks is allowed (test helpers)
/// - std::fs in synchronous (non-async) functions is allowed
/// - internal.rs::ssh_proxy() is the only documented exception for std::process::Command
#[test]
fn application_has_no_blocking_io() {
    let app_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("application");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&app_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();
        let rel_normalized = rel.replace('\\', "/");

        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        // Track context: async fn, spawn_blocking, cfg(unix), cfg(test)
        let mut in_async_fn = false;
        let mut in_spawn_blocking = false;
        let mut in_cfg_unix = false;
        let mut in_cfg_test = false;
        let mut brace_depth: i32 = 0;
        let mut async_fn_start: i32 = -1;
        let mut spawn_blocking_start: i32 = -1;
        let mut cfg_unix_start: i32 = -1;
        let mut cfg_test_start: i32 = -1;

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Track async fn entry
            if (trimmed.contains("async fn ") || trimmed.contains("pub async fn "))
                && !trimmed.starts_with("//")
            {
                in_async_fn = true;
                async_fn_start = brace_depth;
            }
            // Track spawn_blocking entry
            if trimmed.contains("spawn_blocking") {
                in_spawn_blocking = true;
                spawn_blocking_start = brace_depth;
            }
            // Track #[cfg(unix)] entry
            if trimmed.contains("#[cfg(unix)]") {
                in_cfg_unix = true;
                cfg_unix_start = brace_depth;
            }
            // Track #[cfg(test)] entry
            if trimmed.contains("#[cfg(test)]") {
                in_cfg_test = true;
                cfg_test_start = brace_depth;
            }

            // Count braces
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if in_spawn_blocking && brace_depth <= spawn_blocking_start {
                            in_spawn_blocking = false;
                        }
                        if in_cfg_unix && brace_depth <= cfg_unix_start {
                            in_cfg_unix = false;
                        }
                        if in_cfg_test && brace_depth <= cfg_test_start {
                            in_cfg_test = false;
                        }
                        if in_async_fn && brace_depth <= async_fn_start {
                            in_async_fn = false;
                        }
                    }
                    _ => {}
                }
            }

            // Skip comment lines
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }

            // Only check inside async functions
            if !in_async_fn {
                continue;
            }

            // Skip lines inside spawn_blocking (std::fs is allowed there)
            if in_spawn_blocking {
                continue;
            }

            // Skip lines inside #[cfg(unix)] blocks
            if in_cfg_unix {
                continue;
            }

            // Skip lines inside #[cfg(test)] blocks
            if in_cfg_test {
                continue;
            }

            // std::fs usage in async fn outside spawn_blocking
            if line.contains("std::fs::")
                || (line.contains("use std::fs") && !line.contains("use std::fs::path"))
            {
                violations.push(format!(
                    "{rel}:{}: std::fs in async fn outside spawn_blocking — use spawn_blocking: {line}",
                    i + 1
                ));
            }
            // std::net usage
            if line.contains("std::net::TcpStream") || line.contains("std::net::ToSocketAddrs") {
                violations.push(format!(
                    "{rel}:{}: std::net in application/ — use NetworkProbe port: {line}",
                    i + 1
                ));
            }
            // std::process::Command — except internal.rs
            if line.contains("std::process::Command") && !rel_normalized.contains("internal.rs") {
                violations.push(format!(
                    "{rel}:{}: std::process::Command in application/ — use CommandRunner port: {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "application/ async functions must not use blocking I/O outside spawn_blocking:\n{}",
        violations.join("\n")
    );
}

/// No module-level #![allow(dead_code)] in domain/, application/, or infra/ layers.
#[test]
fn no_module_level_dead_code_allows_in_layers() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let layer_dirs = [
        src_dir.join("domain"),
        src_dir.join("application"),
        src_dir.join("infra"),
    ];

    let mut violations: Vec<String> = Vec::new();

    for dir in &layer_dirs {
        for file in collect_rs_files(dir) {
            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };
            let rel = file
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&file)
                .display()
                .to_string();

            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                // Only flag module-level (inner attribute) dead_code suppression
                if trimmed == "#![allow(dead_code)]" {
                    violations.push(format!(
                        "{rel}:{}: module-level #![allow(dead_code)] — use item-level suppression with a comment explaining why",
                        i + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Module-level #![allow(dead_code)] found in architecture layers — use item-level suppression:\n{}",
        violations.join("\n")
    );
}

/// No calls to generate-agent.sh anywhere in the Rust source.
#[test]
fn no_generate_agent_sh_calls() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&src_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            if line.contains("generate-agent.sh") {
                violations.push(format!(
                    "{rel}:{}: generate-agent.sh call found — replaced by Rust generator: {line}",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "generate-agent.sh calls must not exist — use Rust artifact generator:\n{}",
        violations.join("\n")
    );
}

/// Old root-level module imports must not exist anywhere in the source.
#[test]
fn no_old_root_module_imports() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let old_modules = [
        "crate::command_runner::",
        "crate::provisioner::",
        "crate::state::",
        "crate::ssh::",
        "crate::assets::",
    ];

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&src_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();
        let rel_normalized = rel.replace('\\', "/");

        // infra/ files may reference their own module in docs/comments — skip
        if rel_normalized.contains("/infra/") {
            continue;
        }

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            for old_mod in &old_modules {
                if line.contains(old_mod) {
                    violations.push(format!(
                        "{rel}:{}: old root-level module `{old_mod}` (module deleted): {line}",
                        i + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Old root-level module imports found — these modules were deleted:\n{}",
        violations.join("\n")
    );
}

/// Test files must use new module paths, not old root-level paths.
#[test]
fn test_imports_use_new_paths() {
    let tests_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let old_paths = [
        "polis_cli::command_runner::",
        "polis_cli::provisioner::",
        "polis_cli::state::",
        "polis_cli::ssh::",
        "polis_cli::assets::",
        "polis_cli::workspace::",
    ];

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&tests_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();
        let rel_normalized = rel.replace('\\', "/");

        // Skip this file itself — it contains the old path strings as string literals
        // in the old_paths array above, which would cause false positives.
        if rel_normalized.ends_with("architecture.rs") {
            continue;
        }

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            for old_path in &old_paths {
                if line.contains(old_path) {
                    violations.push(format!(
                        "{rel}:{}: old module path `{old_path}` in test: {line}",
                        i + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Test files must use new module paths:\n{}",
        violations.join("\n")
    );
}

/// The workspace/ directory must not exist.
#[test]
fn workspace_directory_removed() {
    let workspace_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("workspace");

    assert!(
        !workspace_dir.exists(),
        "src/workspace/ directory still exists — it should have been deleted in Phase 3a"
    );
}

// ── Property 16: Application layer boundary ──────────────────────────────────

/// application/ must not import from infra/ or output/ layers.
#[test]
fn application_has_no_infra_or_output_imports() {
    let app_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("application");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&app_dir) {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            if line.contains("crate::infra::") || line.contains("crate::output::") {
                violations.push(format!("{rel}:{}: forbidden import: {line}", i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "application/ must not import from infra/ or output/:\n{}",
        violations.join("\n")
    );
}
