//! Ports `core/lib/install.mjs`: "dfw install" / "dfw uninstall" register dfw with the AI coding
//! tools ("hosts") on this machine: Claude Code and GitHub Copilot today (see core/host). Shared
//! by the npx installer (scripts/install.mjs) and the standalone binary (install.sh), so a
//! machine without Node can install everything with one command.

use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::binary::RELEASE_REPO;
use crate::browser::a11y::{axe_status, install_axe, AxeStatus};
use crate::browser::download::install_browser;
use crate::browser::launch::{browser_status, make_temp_dir, BrowserStatus};
use crate::core::error::DfwError;
use crate::core::host::{get_host, supported_hosts, Host, Status, HOSTS};
use crate::core::paths::{plugin_root, resolve};
use crate::core::process::{find_executable, plan_for, run_timed, Ran, Spawn};
use crate::core::util::{out, plugin_manifest, read_json};

/// The marketplace dfw and its skills are installed from.
pub const MARKET: &str = "dfw-marketplace";

fn dim(s: &str) -> String {
    format!("\x1b[2m{s}\x1b[0m")
}
fn ok(s: &str) -> String {
    format!("\x1b[32m{s}\x1b[0m")
}
fn warn(s: &str) -> String {
    format!("\x1b[33m{s}\x1b[0m")
}
fn bold(s: &str) -> String {
    format!("\x1b[1m{s}\x1b[0m")
}

/// `pluginList()`: plugins to install: dfw first, then the design skills it depends on.
pub fn plugin_list() -> Result<Vec<String>, DfwError> {
    let listed = |v: &Value| v.get("dependencies").filter(|d| !d.is_null()).cloned();
    let deps = match plugin_root() {
        Some(root) => match listed(&read_json(&root.join("dfw.json"), Some(Value::Null))?) {
            Some(deps) => Some(deps),
            None => listed(&plugin_manifest(None)?),
        },
        // No plugin folder (the binary installed alone by install.sh): the list it was built with.
        None => listed(&crate::assets::manifest()),
    };
    let mut plugins = vec!["dfw".to_string()];
    plugins.extend(
        deps.as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| d.as_str().map(str::to_string)),
    );
    Ok(plugins)
}

/// How a host CLI (`claude`, `copilot`, a bare name) or dfw itself (an absolute path) is
/// started: a bare name is looked up on PATH, never in the current folder (a cloned repository's
/// `claude.cmd` must not run); a Windows `.cmd` shim goes through cmd.exe with every part quoted.
/// `None` when the program is not found.
pub fn spawn_plan(bin: &str, args: &[String]) -> Option<Spawn> {
    let exe = if Path::new(bin).is_absolute() {
        bin.to_string()
    } else {
        find_executable(bin)?.display().to_string()
    };
    Some(plan_for(&exe, args.to_vec(), cfg!(windows)))
}

/// How long a host CLI call may take: `marketplace add` clones a repository.
const HOST_CLI_TIMEOUT: Duration = Duration::from_secs(300);
/// How long `<cli> --version` may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(15);
/// How long dfw's own `check --setup` may take: the engine download has three minutes inside.
const SELF_TIMEOUT: Duration = Duration::from_secs(600);

fn spawn_sync(bin: &str, args: &[String], env: &[(&str, &Path)], timeout: Duration) -> Ran {
    let Some(plan) = spawn_plan(bin, args) else {
        return Ran {
            error: Some(format!("spawnSync {bin} ENOENT")),
            ..Ran::default()
        };
    };
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.display().to_string()))
        .collect();
    run_timed(&plan, None, &env, timeout)
}

fn has_cli(bin: &str) -> bool {
    spawn_sync(bin, &["--version".to_string()], &[], VERSION_TIMEOUT).status == Some(0)
}

/// `marketplaceAction(listOut, source, name)`: what to do with the marketplace in Claude Code,
/// from "claude plugin marketplace list --json": "add" (not registered), "update" (registered
/// from this source), "readd" (registered from another source, e.g. GitHub while --source is a
/// local clone: update would keep the old one).
pub fn marketplace_action(list_out: &str, source: &str, name: &str) -> &'static str {
    let Ok(list) = serde_json::from_str::<Value>(list_out) else {
        // Older Claude Code: no --json.
        return if list_out.contains(name) {
            "update"
        } else {
            "add"
        };
    };
    let cur = list
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(name));
    let Some(cur) = cur else {
        return "add";
    };
    let norm = |v: &str| {
        let v = v.strip_prefix("https://github.com/").unwrap_or(v);
        let v = v.strip_suffix(".git").unwrap_or(v);
        v.trim_end_matches(['/', '\\']).to_string()
    };
    let known: Vec<String> = ["repo", "path", "url", "directory"]
        .iter()
        .filter_map(|k| cur.get(k).and_then(Value::as_str))
        .map(norm)
        .collect();
    if known.contains(&norm(source))
        || known.contains(&norm(&resolve(source).display().to_string()))
    {
        "update"
    } else {
        "readd"
    }
}

fn supported_list() -> String {
    supported_hosts().join(", ")
}

/// `resolveHosts(host)`: hosts to act on: --host claude|copilot|all, default "auto" = every
/// supported host whose CLI is installed.
pub fn resolve_hosts(host: &str) -> Result<Vec<&'static Host>, DfwError> {
    if host == "all" {
        // Each must have its CLI.
        let mut all = Vec::new();
        for id in supported_hosts() {
            all.extend(resolve_hosts(id)?);
        }
        return Ok(all);
    }
    if host != "auto" {
        let h = get_host(Some(host))?;
        if h.status != Status::Supported {
            return Err(DfwError::new(format!(
                "{} support is planned but not available yet. Supported: {}.",
                h.name,
                supported_list()
            )));
        }
        let cli = h.cli.unwrap_or_default();
        if !has_cli(cli) {
            return Err(DfwError::new(format!(
                "{} CLI (\"{cli}\") not found on PATH.",
                h.name
            )));
        }
        return Ok(vec![h]);
    }
    let found: Vec<&'static Host> = HOSTS
        .iter()
        .filter(|h| h.status == Status::Supported && h.cli.is_some_and(has_cli))
        .collect();
    if found.is_empty() {
        let clis: Vec<&str> = HOSTS
            .iter()
            .filter(|h| h.status == Status::Supported)
            .filter_map(|h| h.cli)
            .collect();
        return Err(DfwError::new(format!(
            "No supported AI coding tool found on PATH ({}). Install Claude Code or GitHub Copilot CLI first, or pass --host.",
            clis.join(", ")
        )));
    }
    Ok(found)
}

/// `runner(quiet)`: runs a command, echoing it unless quiet; a failure is an error unless the
/// call allows it.
struct Runner {
    quiet: bool,
}

impl Runner {
    fn run(&self, bin: &str, args: &[&str], allow_fail: bool) -> Result<Ran, DfwError> {
        self.run_env(bin, args, allow_fail, &[], HOST_CLI_TIMEOUT)
    }

    /// A timeout is an error also when the call allows a failure: nothing can be read from it.
    fn run_env(
        &self,
        bin: &str,
        args: &[&str],
        allow_fail: bool,
        env: &[(&str, &Path)],
        timeout: Duration,
    ) -> Result<Ran, DfwError> {
        let line = format!("{bin} {}", args.join(" "));
        if !self.quiet {
            out(&dim(&format!("  $ {line}")));
        }
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let failed = |msg: &str| {
            let msg = msg.trim();
            DfwError::internal(if msg.is_empty() {
                format!("{line} failed")
            } else {
                format!("{line} failed:\n{msg}")
            })
        };
        let r = spawn_sync(bin, &args, env, timeout);
        if let Some(e) = &r.error {
            return Err(failed(e));
        }
        if r.timed_out {
            return Err(failed(&format!(
                "no answer after {} s; it was stopped",
                timeout.as_secs()
            )));
        }
        if r.status != Some(0) && !allow_fail {
            return Err(failed(if r.stderr.is_empty() {
                &r.stdout
            } else {
                &r.stderr
            }));
        }
        Ok(r)
    }
}

fn step(s: &str) {
    out(&format!("{} {s}", bold("›")));
}

/// `installInHost`: register the marketplace and install dfw + its skills in one host.
/// Returns what the user does next.
fn install_in_host(
    h: &Host,
    run: &Runner,
    scope: &str,
    source: &str,
    plugins: &[String],
) -> Result<&'static str, DfwError> {
    if h.id == "claude" {
        step(&format!("Claude Code: marketplace {MARKET} from {source}"));
        let mut list = run.run("claude", &["plugin", "marketplace", "list", "--json"], true)?;
        if list.status != Some(0) {
            list = run.run("claude", &["plugin", "marketplace", "list"], true)?;
        }
        let action = marketplace_action(&list.stdout, source, MARKET);
        if action == "update" {
            run.run(
                "claude",
                &["plugin", "marketplace", "update", MARKET],
                false,
            )?;
        } else {
            if action == "readd" {
                run.run("claude", &["plugin", "marketplace", "remove", MARKET], true)?;
            }
            run.run(
                "claude",
                &["plugin", "marketplace", "add", source, "--scope", scope],
                false,
            )?;
        }
        step(&format!(
            "Claude Code plugins ({scope} scope): {}",
            plugins.join(", ")
        ));
        // dfw declares the skills as dependencies; installing each explicitly keeps older
        // Claude Code versions working.
        for p in plugins {
            let plugin = format!("{p}@{MARKET}");
            run.run(
                "claude",
                &["plugin", "install", &plugin, "--scope", scope],
                p != "dfw",
            )?;
        }
        return Ok("Restart Claude Code (or run /reload-plugins), open a project and type /dfw.");
    }
    if h.id == "copilot" {
        step(&format!(
            "GitHub Copilot: marketplace {MARKET} from {source}"
        ));
        // An explicit --source replaces a marketplace registered from somewhere else ("add"
        // alone would fail and keep it).
        if source != RELEASE_REPO {
            run.run(
                "copilot",
                &["plugin", "marketplace", "remove", MARKET],
                true,
            )?;
        }
        // Already added -> update below.
        run.run("copilot", &["plugin", "marketplace", "add", source], true)?;
        run.run(
            "copilot",
            &["plugin", "marketplace", "update", MARKET],
            true,
        )?;
        step(&format!("GitHub Copilot plugins: {}", plugins.join(", ")));
        for p in plugins {
            let plugin = format!("{p}@{MARKET}");
            let r = run.run("copilot", &["plugin", "install", &plugin], true)?;
            if r.status != Some(0) {
                run.run("copilot", &["plugin", "update", p], p != "dfw")?;
            }
        }
        link_copilot_cli();
        return Ok("Restart GitHub Copilot (CLI session or VS Code window), open a project and ask for UI work (the dfw skill loads itself).");
    }
    Err(DfwError::internal(format!("No installer for {}", h.name)))
}

/// Copilot does not put plugin bin/ on PATH: link ~/.local/bin/dfw to the installed launcher
/// (POSIX only). Any failure leaves things as they are.
fn link_copilot_cli() {
    #[cfg(unix)]
    {
        let Some(target) = HOSTS
            .iter()
            .find(|h| h.id == "copilot")
            .and_then(Host::dfw_plugin_dir)
            .map(|dir| dir.join("bin").join("dfw"))
        else {
            return;
        };
        let link = env::home_dir()
            .unwrap_or_default()
            .join(".local")
            .join("bin")
            .join("dfw");
        if link.exists() {
            let Ok(current) = fs::read_link(&link) else {
                return; // a real file (e.g. install.sh binary link target): leave it
            };
            if current != target && current.exists() {
                return; // points at another working dfw
            }
        }
        if let Some(dir) = link.parent() {
            if fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        // Kept from Node: `fs.rmSync(link, { force: true })` leaves a link whose target is gone
        // where it is, the new link then fails, and the stale one stays.
        if link.exists() {
            let _ = fs::remove_file(&link);
        }
        let _ = std::os::unix::fs::symlink(&target, &link);
    }
}

/// `selfCommand()`: this dfw, to run again in a child process.
fn self_command() -> Result<String, DfwError> {
    Ok(env::current_exe()?.display().to_string())
}

/// What `install` does (`install({ host, scope, source, runtime, quiet })`). Node's `binary`
/// option (download the dfw binary) has no meaning here: this is the binary.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub host: String,
    pub scope: String,
    /// The marketplace: a GitHub repository or a local clone.
    pub source: String,
    /// Also set up what renders and checks need (browser, axe-core, the Impeccable engine).
    pub runtime: bool,
    pub quiet: bool,
}

/// `install(opts)`: register dfw and its skills with the hosts, set up the runtime, run doctor.
pub fn install(opts: &InstallOptions) -> Result<(), DfwError> {
    let run = Runner { quiet: opts.quiet };
    let hosts = resolve_hosts(&opts.host)?;
    let names: Vec<&str> = hosts.iter().map(|h| h.name).collect();
    step(&format!("Hosts: {} · dfw binary", names.join(", ")));
    let plugins = plugin_list()?;
    let mut next = Vec::new();
    for h in hosts {
        next.push(install_in_host(
            h,
            &run,
            &opts.scope,
            &opts.source,
            &plugins,
        )?);
    }
    let me = self_command()?;
    if opts.runtime {
        match browser_status() {
            BrowserStatus::Installed(b) => step(&format!(
                "Browser for screenshots and layout checks: {}",
                b.path.display()
            )),
            BrowserStatus::Missing { .. } => {
                step("Headless Chrome for screenshots and layout checks (no Chrome/Edge/Chromium found)");
                if let Err(e) = install_browser(&|m| out(&dim(&format!("  {m}")))) {
                    out(&warn(&format!("  skipped: {e}")));
                }
            }
        }
        if matches!(axe_status(), AxeStatus::Missing { .. }) {
            step("axe-core for the accessibility check (pinned, integrity-checked)");
            if let Err(e) = install_axe(&|m| out(&dim(&format!("  {m}")))) {
                out(&warn(&format!("  skipped: {e}")));
            }
        }
        step("Impeccable engine (detect only, telemetry and update checks off)");
        let tmp = make_temp_dir(&env::temp_dir(), "dfw-setup-")?;
        fs::create_dir_all(tmp.join(".dfw"))?;
        let ran = run.run_env(
            &me,
            &["check", "--setup"],
            true,
            &[("CLAUDE_PROJECT_DIR", &tmp)],
            SELF_TIMEOUT,
        );
        let _ = fs::remove_dir_all(&tmp);
        ran?;
    }
    step("Doctor");
    let d = run.run(&me, &["doctor"], true)?;
    let indented: Vec<String> = d.stdout.split('\n').map(|l| format!("  {l}")).collect();
    out(&indented.join("\n"));
    out(&format!("\n{} {}", ok("Done."), next.join(" ")));
    Ok(())
}

/// `uninstall({ host, scope, quiet })`: remove dfw and its skills from the hosts.
pub fn uninstall(host: &str, scope: &str, quiet: bool) -> Result<(), DfwError> {
    let run = Runner { quiet };
    for h in resolve_hosts(host)? {
        if h.id == "claude" {
            for p in plugin_list()? {
                let plugin = format!("{p}@{MARKET}");
                run.run(
                    "claude",
                    &["plugin", "uninstall", &plugin, "--scope", scope],
                    true,
                )?;
            }
            run.run("claude", &["plugin", "marketplace", "remove", MARKET], true)?;
        } else if h.id == "copilot" {
            for p in plugin_list()? {
                run.run("copilot", &["plugin", "uninstall", &p], true)?;
            }
            run.run(
                "copilot",
                &["plugin", "marketplace", "remove", MARKET],
                true,
            )?;
        }
        out(&ok(&format!("{}: removed dfw and its skills.", h.name)));
    }
    out("Project files (.dfw/, design/) are untouched.");
    Ok(())
}

#[cfg(test)]
mod tests {
    //! tests/unit/install-paths.test.mjs, the install.mjs cases (the launcher, checks and
    //! upstream bot cases test JavaScript that stays). `install` and `uninstall` themselves run
    //! against fake host CLIs in tests/golden/browser.golden.mjs.
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    /// install: a host CLI is looked up on PATH, an absolute dfw runs as it is (the Windows
    /// quoting is tested in core::process)
    #[test]
    fn spawn_plan_resolves_bare_names_only() {
        assert_eq!(
            spawn_plan("/opt/dfw/bin/dfw", &args(&["doctor"])),
            Some(Spawn::direct("/opt/dfw/bin/dfw", args(&["doctor"])))
        );
        assert_eq!(spawn_plan("dfw-no-such-host-cli", &[]), None);
        if cfg!(unix) {
            let sh = spawn_plan("sh", &args(&["a b"])).unwrap();
            assert!(Path::new(&sh.cmd).is_absolute() && sh.args == args(&["a b"]) && !sh.shell);
        }
    }

    /// install: marketplace is re-added only when registered from another source
    #[test]
    fn marketplace_is_readded_only_from_another_source() {
        let repo = "tantm97/ai-frontend-design-workflow";
        let list = r#"[{"name":"other","source":"github","repo":"a/b"},{"name":"dfw-marketplace","source":"github","repo":"tantm97/ai-frontend-design-workflow"}]"#;
        let action = |list: &str, source: &str| marketplace_action(list, source, MARKET);
        assert_eq!(action(list, repo), "update");
        assert_eq!(
            action(
                list,
                "https://github.com/tantm97/ai-frontend-design-workflow.git"
            ),
            "update"
        );
        assert_eq!(action(list, "./my-clone"), "readd");
        assert_eq!(action("[]", "./my-clone"), "add");
        let local = serde_json::json!([{ "name": "dfw-marketplace", "source": "directory", "path": resolve("my-clone").display().to_string() }]).to_string();
        assert_eq!(action(&local, "./my-clone"), "update");
        assert_eq!(action(&local, repo), "readd");
        // Older Claude Code without --json: the text listing.
        assert_eq!(
            action("Configured marketplaces:\n  dfw-marketplace\n", "x"),
            "update"
        );
        assert_eq!(action("Configured marketplaces:\n", "x"), "add");
        assert_eq!(action("", "x"), "add");
        assert_eq!(action("42", "x"), "add");
    }

    #[test]
    fn unknown_and_planned_hosts_are_refused() {
        let err = resolve_hosts("cursor").unwrap_err();
        assert_eq!(
            err.message(),
            "Cursor support is planned but not available yet. Supported: claude, copilot."
        );
        assert_eq!(err.report(), err.message(), "a user's choice, not an internal error");
        assert!(resolve_hosts("nope")
            .unwrap_err()
            .message()
            .starts_with("Unknown host \"nope\""));
    }

    #[test]
    fn a_missing_program_is_reported_like_spawn_sync() {
        let run = Runner { quiet: true };
        let err = run
            .run("dfw-no-such-host-cli", &["plugin", "list"], true)
            .err()
            .unwrap();
        assert_eq!(
            err.message(),
            "dfw-no-such-host-cli plugin list failed:\nspawnSync dfw-no-such-host-cli ENOENT"
        );
        if cfg!(unix) {
            let err = run
                .run("sh", &["-c", "echo no >&2; exit 3"], false)
                .err()
                .unwrap();
            assert_eq!(err.message(), "sh -c echo no >&2; exit 3 failed:\nno");
            // A host CLI that hangs is stopped, also when the call allows a failure.
            let err = run
                .run_env(
                    "sh",
                    &["-c", "sleep 30"],
                    true,
                    &[],
                    Duration::from_millis(200),
                )
                .err()
                .unwrap();
            assert!(
                err.message()
                    .starts_with("sh -c sleep 30 failed:\nno answer after "),
                "{}",
                err.message()
            );
            assert_eq!(
                run.run("sh", &["-c", "echo out; exit 3"], true)
                    .unwrap()
                    .stdout,
                "out\n"
            );
        }
    }
}
