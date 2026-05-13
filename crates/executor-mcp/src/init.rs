//! v1.3 Stream A: `executor-mcp init` subcommand.
//!
//! Steps (per DESIGN.md):
//!   1. Refuse if `./.local/config.toml` already exists unless `--force`.
//!   2. Generate fresh secp256k1 key (alloy).
//!   3. Store hex key in OS keychain (service="onchain-strategy-mcp",
//!      account="default").
//!   4. Write `./.local/config.toml` from the embedded `.local.example`
//!      template with `[signer] backend = "keychain", key_id = "default"`
//!      and absolute paths filled in.
//!   5. Write `./.local/policy.toml` from the embedded template verbatim.
//!   6. Print burner address + the exact `claude mcp add` command. If
//!      `claude` is on PATH, ask `Y/n` to run it.
//!   7. Exit 0.
//!
//! User-facing output goes to stdout; the workspace-wide `print_stdout`
//! lint is opted out at the module level — this is the only binary entry
//! that prints (the stdio MCP server keeps strict JSON-RPC discipline).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result, bail};
use executor_signer::{
    KEYCHAIN_SERVICE, generate_burner, predicted_delegate_address, store_in_keychain,
};

/// Embedded `.local.example/config.toml` — kept in lockstep at build time.
const CONFIG_TEMPLATE: &str = include_str!("../../../.local.example/config.toml");
/// Embedded `.local.example/policy.toml` — kept in lockstep at build time.
const POLICY_TEMPLATE: &str = include_str!("../../../.local.example/policy.toml");

const KEY_ID: &str = "default";

pub struct InitOptions {
    pub force: bool,
    /// Skip interactive prompts (used in CI / smoke tests).
    pub non_interactive: bool,
}

pub fn run(opts: InitOptions) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let local_dir = cwd.join(".local");
    let config_path = local_dir.join("config.toml");
    let policy_path = local_dir.join("policy.toml");

    // 1. Refuse if already exists.
    if config_path.exists() && !opts.force {
        bail!(
            "{} already exists — pass --force to overwrite",
            config_path.display()
        );
    }

    fs::create_dir_all(&local_dir)
        .with_context(|| format!("creating {}", local_dir.display()))?;

    // 2. Generate fresh burner key.
    let (hex_key, address) = generate_burner();

    // 3. Store in OS keychain.
    store_in_keychain(KEY_ID, &hex_key).map_err(|err| {
        anyhow::anyhow!(
            "failed to store private key in OS keychain: {err}\n\
             on Linux this usually means libsecret / gnome-keyring is missing — try:\n  \
             sudo apt-get install -y libsecret-1-0 gnome-keyring"
        )
    })?;
    // Wipe the key from this process's memory ASAP.
    drop(hex_key);

    // 4. Materialise config.toml.
    let state_db_path = local_dir.join("state.db");
    let config_contents = render_config(
        CONFIG_TEMPLATE,
        &state_db_path,
        &policy_path,
        &address,
    );
    write_file(&config_path, &config_contents)?;

    // 5. Materialise policy.toml.
    write_file(&policy_path, POLICY_TEMPLATE)?;

    // 6. Print summary + `claude mcp add` instructions.
    println!();
    println!("onchain-strategy-mcp initialised.");
    println!();
    println!("  burner address : {address}");
    println!("  keychain entry : service={KEYCHAIN_SERVICE}, account={KEY_ID}");
    println!("  config         : {}", config_path.display());
    println!("  policy         : {}", policy_path.display());
    println!();
    println!("Fund the burner address with a small amount of ETH on Base before running");
    println!("any non-read-only strategy.");
    println!();

    let delegate_addr = predicted_delegate_address();
    println!("EIP-7702 batching (optional, one-time per chain):");
    println!("  predicted delegate : {delegate_addr}");
    println!(
        "  When you're ready to run 7702 batches, deploy the delegate (free; one-time per chain):"
    );
    println!("    executor-mcp deploy-delegate --rpc-url <your-rpc>");
    println!(
        "  Or skip — the runtime stays a one-tx-at-a-time executor until you do."
    );
    println!();

    let claude_cmd = "claude mcp add osmcp -- npx onchain-strategy-mcp serve";
    println!("Register with Claude Code:");
    println!("  {claude_cmd}");
    println!();

    if !opts.non_interactive && claude_on_path() {
        print!("Run it now? [Y/n] ");
        io::stdout().flush().ok();
        let mut line = String::new();
        let stdin = io::stdin();
        stdin.lock().read_line(&mut line).ok();
        let trimmed = line.trim().to_lowercase();
        if trimmed.is_empty() || trimmed == "y" || trimmed == "yes" {
            let status = Command::new("claude")
                .args([
                    "mcp",
                    "add",
                    "osmcp",
                    "--",
                    "npx",
                    "onchain-strategy-mcp",
                    "serve",
                ])
                .status();
            match status {
                Ok(s) if s.success() => println!("Registered with Claude Code."),
                Ok(s) => eprintln!("`claude mcp add` exited with status: {s}"),
                Err(err) => eprintln!("failed to spawn `claude`: {err}"),
            }
        }
    }

    Ok(())
}

fn claude_on_path() -> bool {
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    std::env::split_paths(&path).any(|dir| dir.join("claude").is_file())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

/// Apply v1.3 substitutions over the embedded config template:
///   - replace the `[state].path` placeholder with the absolute state.db path
///   - replace the `[policy].path` placeholder with the absolute policy.toml path
///   - replace `simulation_from` with the freshly minted burner address
///   - rewrite the `[signer]` section to `backend = "keychain"`,
///     `key_id = "default"`
fn render_config(
    template: &str,
    state_db_path: &Path,
    policy_path: &Path,
    burner_address: &str,
) -> String {
    let state_str = path_to_toml_str(state_db_path);
    let policy_str = path_to_toml_str(policy_path);

    let mut out = String::with_capacity(template.len() + 256);
    let mut in_signer = false;
    for line in template.lines() {
        let trimmed = line.trim_start();

        // Detect [signer] section to rewrite its body.
        if trimmed.starts_with('[') {
            in_signer = trimmed == "[signer]";
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if in_signer {
            // Skip the legacy `private_key_env` line and the receipt_timeout
            // line — we re-emit the whole section below the first time we
            // hit a non-key line. But to keep this simple, replace this
            // section entirely on the FIRST signer header occurrence by
            // emitting nothing here and letting the closing [next_section]
            // header trigger the flush.
            // Easier strategy: passthrough the line UNLESS it sets
            // private_key_env / receipt_timeout_ms, and inject backend +
            // key_id lines for the first encountered key.
            if trimmed.starts_with("private_key_env") {
                // Replace with keychain backend lines.
                out.push_str("backend = \"keychain\"\n");
                out.push_str(&format!("key_id = \"{KEY_ID}\"\n"));
                continue;
            }
            // Preserve receipt_timeout_ms and comments verbatim.
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // [state].path placeholder rewrite.
        if line.contains("/REPLACE/WITH/ABSOLUTE/PATH/state.db") {
            out.push_str(&format!("path = {state_str}\n"));
            continue;
        }
        // [policy].path placeholder rewrite.
        if line.contains("/REPLACE/WITH/ABSOLUTE/PATH/.local/policy.toml") {
            out.push_str(&format!("path = {policy_str}\n"));
            continue;
        }
        // simulation_from default placeholder (anvil-0 / zero addr) → burner.
        if trimmed.starts_with("simulation_from") {
            out.push_str(&format!("simulation_from = \"{burner_address}\"\n"));
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }
    out
}

fn path_to_toml_str(p: &Path) -> String {
    // Simple TOML string encoding — escape backslashes and double quotes.
    let s: String = p
        .to_string_lossy()
        .chars()
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            other => vec![other],
        })
        .collect();
    format!("\"{s}\"")
}

// Allow tests to import the private renderer.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_config_rewrites_signer_to_keychain() {
        let path = std::path::PathBuf::from("/tmp/foo.db");
        let pol = std::path::PathBuf::from("/tmp/policy.toml");
        let out = render_config(CONFIG_TEMPLATE, &path, &pol, "0xABC");
        assert!(out.contains("backend = \"keychain\""));
        assert!(out.contains("key_id = \"default\""));
        assert!(!out.contains("private_key_env"));
        assert!(out.contains("simulation_from = \"0xABC\""));
        assert!(out.contains("/tmp/foo.db"));
        assert!(out.contains("/tmp/policy.toml"));
    }
}
