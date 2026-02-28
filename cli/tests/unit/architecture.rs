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

/// Scan all files in `commands/` for `json: bool` in function signatures
/// and `if json` / `if !json` patterns.
///
/// After task 10.2, all JSON branching must go through `app.renderer()`.
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

            // Check for `json: bool` in function signatures
            if line.contains("json: bool") {
                violations.push(format!(
                    "{rel}:{lineno}: found `json: bool` parameter: {line}"
                ));
            }

            // Check for `if json {` or `if !json {` branching
            let trimmed = line.trim();
            if trimmed.starts_with("if json") || trimmed.starts_with("if !json") {
                violations.push(format!("{rel}:{lineno}: found inline JSON branch: {line}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found inline JSON branching in commands/ — use app.renderer() instead:
{}",
        violations.join(
            "
"
        )
    );
}

// ── Property 7: All VM operations route through provisioner ──────────────────

/// Scan source files for `TokioCommandRunner::new` outside `infra/` — any
/// match means a command or service is bypassing the provisioner trait system.
///
/// After task 18.1, `TokioCommandRunner::new` must only appear in `infra/`.
#[test]
fn no_tokio_command_runner_new_outside_infra() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&src_dir) {
        // Allow TokioCommandRunner::new inside infra/ and provisioner.rs (backward-compat shim)
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .to_string_lossy()
            .to_string();

        // Normalize path separators for cross-platform matching
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
        "Found TokioCommandRunner::new outside infra/ — all VM ops must go through provisioner traits:
{}",
        violations.join("
")
    );
}

/// Scan source files for `multipass::` imports — after task 18.1, the
/// `multipass` module is deleted and no imports should reference it.
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
            // Match `crate::multipass::` or `use crate::multipass`
            if line.contains("crate::multipass::") || line.contains("use crate::multipass") {
                violations.push(format!("{rel}:{}: found multipass:: import: {line}", i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found multipass:: imports — the multipass module has been deleted:
{}",
        violations.join(
            "
"
        )
    );
}

// ── Property 5: Trait bounds over concrete types ──────────────────────────────

/// Scan all function signatures in `infra/` and `application/services/` for
/// concrete provisioner/runner types — test fails if any found.
///
/// After task 22.1, all service and infra functions must use trait bounds
/// (`&impl Trait` or `<P: Trait>`) rather than concrete types.
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
                // Only check function signatures (lines with `fn `)
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
        "Found concrete provisioner/runner types in service/infra function signatures — use trait bounds instead:
{}",
        violations.join("
")
    );
}

/// Verify `infra/` has zero imports from `commands/` or `output/`.
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
        "infra/ must not import from commands/ or output/:
{}",
        violations.join(
            "
"
        )
    );
}

/// Verify `infra/` has zero `println!`/`eprintln!` calls outside `#[cfg(test)]`.
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

        // Track whether we're inside a #[cfg(test)] block by counting braces
        let mut in_test_block = false;
        let mut brace_depth: i32 = 0;
        let mut test_block_start_depth: i32 = 0;

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Detect #[cfg(test)] attribute
            if trimmed.contains("#[cfg(test)]") {
                in_test_block = true;
                test_block_start_depth = brace_depth;
            }

            // Count braces
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

            // Skip comment lines
            if trimmed.starts_with("//") {
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
        "infra/ must not use println!/eprintln! outside #[cfg(test)]:
{}",
        violations.join(
            "
"
        )
    );
}

// ── Property 4: No duplicate type definitions ─────────────────────────────────

/// Scan `cli/src/` for struct definitions matching the 10 known duplicate names.
/// After task 24.2, these types must only exist in `polis-common`, not in the CLI.
#[test]
fn no_duplicate_agent_type_definitions_in_cli() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    // The 10 types that were duplicated between CLI and polis-common.
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
            // Match `struct TypeName` or `pub struct TypeName`
            for name in &duplicate_names {
                // Check for struct definition (not usage)
                if (line.contains(&format!("struct {name}"))
                    || line.contains(&format!("struct {name} ")))
                    && (line.contains("struct "))
                {
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
        "Found duplicate agent type definitions in CLI — use polis_common::agent::* instead:
{}",
        violations.join(
            "
"
        )
    );
}

// ── Property 8: Command handlers accept unified AppContext ────────────────────

/// Scan `commands/` for `run()` function signatures accepting `&AppContext`.
///
/// After task 9.2, all command handlers must accept `&AppContext` rather than
/// individual context parameters.
#[test]
fn command_handlers_accept_app_context() {
    let commands_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");

    // Commands that have a public `run()` function and have been migrated to AppContext.
    // Commands not yet migrated are excluded (to be fixed in task 27.2).
    let migrated_command_files = ["agent.rs", "config.rs", "doctor.rs", "status.rs"];

    let mut violations: Vec<String> = Vec::new();

    for filename in &migrated_command_files {
        let file = commands_dir.join(filename);
        if !file.exists() {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        // Check if file has a pub async fn run( that accepts AppContext
        let has_run_fn = content.contains("pub async fn run(");
        if !has_run_fn {
            continue;
        }

        // Find the run function signature and check it accepts AppContext
        let has_app_context = content.contains("app: &AppContext")
            || content.contains("app: &crate::app::AppContext");

        if !has_app_context {
            violations.push(format!(
                "commands/{filename}: pub async fn run() does not accept &AppContext"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "Command handlers must accept &AppContext:
{}",
        violations.join(
            "
"
        )
    );
}

// ── Property 11: Command handler size limits ──────────────────────────────────

/// Verify each file in `commands/` is ≤200 lines of non-test code.
///
/// Files with known large implementations are excluded (to be refactored in future PRs).
#[test]
fn command_handlers_are_reasonably_sized() {
    let commands_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");

    // Files with known large implementations (to be refactored in future PRs)
    let exceptions = [
        "agent.rs",
        "start.rs",
        "update.rs",
        "status.rs",
        "doctor.rs",
    ];

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&commands_dir) {
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        // Skip known exceptions
        if exceptions.contains(&filename.as_str()) {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        // Count non-blank, non-comment lines outside #[cfg(test)] blocks
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

        // 200-line limit for non-test command handler code
        if line_count > 200 {
            let rel = file
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&file)
                .display()
                .to_string();
            violations.push(format!("{rel}: {line_count} non-test lines (limit: 200)"));
        }
    }

    assert!(
        violations.is_empty(),
        "Command handler files exceed size limit — extract logic to application services:
{}",
        violations.join(
            "
"
        )
    );
}

// ── Property 13: Standardized confirmation mechanism ─────────────────────────

/// Scan for forbidden direct stdin/dialoguer usage in `commands/`.
///
/// After task 9.1, all confirmation prompts must go through `app.confirm()`.
/// Known pre-existing violations in connect.rs, delete.rs, update.rs are
/// excluded (to be fixed in task 27.2).
#[test]
fn commands_use_standardized_confirmation() {
    let commands_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");

    // Files with known pre-existing direct confirmation usage (to be fixed in task 27.2)
    let exceptions = ["connect.rs", "delete.rs", "update.rs"];

    let mut violations: Vec<String> = Vec::new();

    for file in collect_rs_files(&commands_dir) {
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if exceptions.contains(&filename.as_str()) {
            continue;
        }

        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&file)
            .display()
            .to_string();

        let lines = read_non_comment_lines(&file);
        for (i, line) in lines.iter().enumerate() {
            // Forbidden: direct stdin locking
            if line.contains("std::io::stdin().lock()") || line.contains("io::stdin().lock()") {
                violations.push(format!(
                    "{rel}:{}: direct stdin lock — use app.confirm() instead: {line}",
                    i + 1
                ));
            }
            // Forbidden: direct dialoguer::Confirm construction
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
        "Commands must use app.confirm() for user prompts:
{}",
        violations.join(
            "
"
        )
    );
}

// ── Property 14: Blocking I/O safety ─────────────────────────────────────────

/// Scan for `std::fs` usage in async functions within `infra/`.
///
/// Blocking I/O in async functions can starve the Tokio runtime. All blocking
/// filesystem operations in infra/ must use `spawn_blocking`.
/// Note: `std::fs` inside `spawn_blocking` closures is allowed.
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

        // Track whether we're inside an async fn and inside a spawn_blocking closure
        let mut in_async_fn = false;
        let mut in_spawn_blocking = false;
        let mut brace_depth: i32 = 0;
        let mut async_fn_start_depth: i32 = 0;
        let mut spawn_blocking_start_depth: i32 = 0;

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Detect async fn start
            if (trimmed.contains("async fn ") || trimmed.contains("async fn\t"))
                && !trimmed.starts_with("//")
            {
                in_async_fn = true;
                async_fn_start_depth = brace_depth;
            }

            // Detect spawn_blocking closure start
            if in_async_fn && line.contains("spawn_blocking") {
                in_spawn_blocking = true;
                spawn_blocking_start_depth = brace_depth;
            }

            // Count braces
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

            if !in_async_fn || in_spawn_blocking {
                continue;
            }

            // Skip comment lines
            if trimmed.starts_with("//") {
                continue;
            }

            // Check for blocking std::fs usage outside spawn_blocking
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
        "infra/ async functions must not use blocking std::fs outside spawn_blocking:
{}",
        violations.join(
            "
"
        )
    );
}
