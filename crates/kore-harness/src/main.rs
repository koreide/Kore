//! `kore-harness`: drive the Kore desktop app against a local kind cluster.
//!
//! ```text
//! kore-harness cluster up                      # kind clusters + fixtures (isolated kubeconfig)
//! kore-harness run                             # Layer 2: every app scenario, in the background
//! kore-harness run -f pods                     # Layer 2: matching scenarios
//! kore-harness run --layer os <name>...        # Layer 3: named OS-input scenarios (takes over input)
//! kore-harness compare <ref-run> <new-run>     # visual regression report
//! kore-harness launch / do ...                 # interactive driving of a running app
//! kore-harness mcp                             # MCP server (stdio) for driving the app from an agent
//! ```
//!
//! The app binary must be built with `--features test-harness` (the control server).

mod compare;
mod driver;
mod mcp;
mod os;
mod report;
mod scenario;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use driver::{App, LaunchConfig, Layer, TextQuery};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "kore-harness", about = "Black-box harness for the Kore desktop app")]
struct Cli {
    /// Repository root (defaults to the workspace this binary was built from).
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage the kind clusters used by scenarios (up | seed | down | status).
    Cluster { action: String },
    /// Run scenarios against an app and write a run directory with screenshots + report.
    Run {
        /// App binary (default target/release/kore, built with `--features test-harness`).
        #[arg(long)]
        binary: Option<PathBuf>,
        /// `app` (Layer 2, default): background, in-app input. `os` (Layer 3): real
        /// OS input from `harness/scenarios/os/`; needs scenario names (or `--all`).
        #[arg(long, value_enum, default_value_t = Layer::App)]
        layer: Layer,
        /// Scenario file or directory.
        #[arg(long)]
        scenarios: Option<PathBuf>,
        /// Only scenarios whose name contains, or tags equal, any of these.
        #[arg(short, long)]
        filter: Vec<String>,
        /// Layer 3 scenario names (exact or substring), as positional arguments.
        names: Vec<String>,
        /// Layer 3: run every OS scenario (otherwise names are required).
        #[arg(long)]
        all: bool,
        /// Layer 3: per-scenario hard timeout in seconds (input is released and the app killed).
        #[arg(long, default_value_t = 60)]
        timeout: u64,
        /// Output directory (default harness/runs/<timestamp>).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Leave the app running after the run.
        #[arg(long)]
        keep_open: bool,
    },
    /// Visual regression between two runs (known-good reference first).
    Compare { baseline: PathBuf, candidate: PathBuf, #[arg(long)] out: Option<PathBuf> },
    /// OCR a PNG and print recognized text with pixel boxes.
    Ocr { image: PathBuf },
    /// Launch an app in the harness sandbox (background, Layer 2) and leave it running.
    Launch {
        #[arg(long)]
        binary: Option<PathBuf>,
    },
    /// Drive an already-running app one action at a time (Layer 2, through its control socket).
    Do {
        /// Control socket printed by `launch`.
        #[arg(long)]
        socket: PathBuf,
        #[command(subcommand)]
        action: DoAction,
    },
    /// MCP server on stdio: launch / snapshot / click / type / key / action / wait / screenshot / quit.
    Mcp {
        #[arg(long)]
        binary: Option<PathBuf>,
    },
    /// Serve a fake OpenAI/Anthropic/Ollama-compatible streaming API for AI scenarios.
    MockAi {
        #[arg(long, default_value_t = 47900)]
        port: u16,
    },
}

#[derive(Subcommand)]
enum DoAction {
    /// Print visible text (OCR) with screen coordinates.
    Text,
    Screenshot { out: PathBuf },
    Click { text: String, #[arg(long, default_value_t = 0)] nth: usize, #[arg(long)] double: bool },
    ClickAt { x: f64, y: f64 },
    Key { chord: String },
    Type { text: String },
    Scroll { x: f64, y: f64, dy: i32 },
    Wait { text: String, #[arg(long, default_value_t = 10.0)] timeout: f64 },
    /// Print the automation snapshot.
    Snapshot,
    /// Print frame stats.
    Frames { #[arg(long)] reset: bool },
}

fn repo_root(cli: &Cli) -> PathBuf {
    cli.root.clone().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
    })
}

pub(crate) fn launch_config(root: &Path, binary: Option<PathBuf>, home: PathBuf, layer: Layer, socket: PathBuf, log: PathBuf) -> Result<LaunchConfig> {
    let binary = binary.unwrap_or_else(|| root.join("target/release/kore"));
    // The app runs with its sandbox as cwd, so a relative path must be resolved here.
    let binary = binary.canonicalize().unwrap_or(binary);
    if !binary.exists() {
        bail!("app binary {} not found (run `cargo build --release -p kore-app --features test-harness`)", binary.display());
    }
    let kubeconfig = root.join("harness/.kube/config");
    if !kubeconfig.exists() {
        bail!("{} missing; run `kore-harness cluster up`", kubeconfig.display());
    }
    Ok(LaunchConfig {
        binary,
        kubeconfig,
        home,
        socket,
        layer,
        extra_env: vec![],
        log_path: log,
    })
}

fn sh(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("command failed: {cmd:?}");
    }
    Ok(())
}

/// Runs hold a lock so concurrent sessions queue instead of colliding:
/// - `.gui.lock` for Layer 3 (real input: only one session may own the mouse and keyboard);
/// - `.app.lock` for Layer 2 (scenarios share the kind clusters' state).
fn run_lock(root: &Path, name: &str) -> Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    let dir = root.join("harness/runs");
    std::fs::create_dir_all(&dir)?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(name))?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        eprintln!("waiting for another harness session to release {name}…");
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            bail!("failed to acquire {name}");
        }
    }
    Ok(f)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = repo_root(&cli);
    match cli.cmd {
        Cmd::Cluster { action } => sh(Command::new(root.join("harness/cluster/cluster.sh")).arg(action)),
        Cmd::Run { binary, layer, scenarios, mut filter, names, all, timeout, out, keep_open } => {
            let default_dir = match layer {
                Layer::App => root.join("harness/scenarios"),
                Layer::Os => root.join("harness/scenarios/os"),
            };
            let scenarios_dir = scenarios.unwrap_or(default_dir);
            if layer == Layer::Os && names.is_empty() && filter.is_empty() && !all {
                bail!(
                    "Layer 3 takes over the mouse and keyboard: name the scenarios to run \
                     (`kore-harness run --layer os <name>...`), or pass --all"
                );
            }
            filter.extend(names);
            let list = scenario::load_dir(&scenarios_dir, &filter)?;
            if list.is_empty() {
                bail!("no scenarios matched in {}", scenarios_dir.display());
            }
            if let Some(bad) = list.iter().find(|s| s.layer != layer) {
                bail!("scenario {} is layer {:?}; run it with --layer {:?}", bad.name, bad.layer, bad.layer);
            }
            let out = out.unwrap_or_else(|| {
                root.join("harness/runs").join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string())
            });
            match layer {
                Layer::App => {
                    let _lock = run_lock(&root, ".app.lock")?;
                    let r = run(&root, binary, layer, &list, &out, keep_open);
                    // Layer 2 must never have touched the user's input devices.
                    assert_eq!(os::input::posted(), 0, "Layer 2 run posted OS input events");
                    r
                }
                Layer::Os => {
                    let _lock = run_lock(&root, ".gui.lock")?;
                    os::guard::arm(Duration::from_secs(timeout));
                    os::input::enable();
                    let r = run(&root, binary, layer, &list, &out, keep_open);
                    os::guard::disarm();
                    r
                }
            }
        }
        Cmd::Compare { baseline, candidate, out } => {
            let out = out.unwrap_or_else(|| candidate.join("compare"));
            let results = compare::compare_runs(&baseline, &candidate, &out)?;
            let bad: Vec<_> = results.iter().filter(|r| !r.ok).collect();
            for r in &results {
                println!(
                    "{} {:<28} {:<28} {:>6.2}% {}",
                    if r.ok { "✓" } else { "✗" },
                    r.scenario,
                    r.name,
                    r.mismatch * 100.0,
                    r.note
                );
            }
            println!("\n{}/{} within tolerance — report: {}", results.len() - bad.len(), results.len(), out.join("index.html").display());
            Ok(())
        }
        Cmd::Ocr { image } => {
            for b in os::ocr::recognize(&image)? {
                println!("{:>6.0},{:>6.0} {:>5.0}x{:<4.0} {:.2}  {}", b.x, b.y, b.w, b.h, b.confidence, b.text);
            }
            Ok(())
        }
        Cmd::Launch { binary } => {
            let sandbox = root.join("harness/runs/interactive");
            let socket = driver::socket_path("interactive");
            let cfg = launch_config(&root, binary, sandbox.join("home"), Layer::App, socket.clone(), sandbox.join("app.log"))?;
            let a = App::launch(&cfg, &sandbox.join("scratch"))?;
            println!("pid={} socket={} log={}", a.pid, socket.display(), sandbox.join("app.log").display());
            a.detach();
            Ok(())
        }
        Cmd::Do { socket, action } => {
            let scratch = root.join("harness/runs/interactive/scratch");
            let mut a = App::attach(0, &socket, Layer::App, &scratch)?;
            let r = do_action(&mut a, action);
            a.detach();
            r
        }
        Cmd::Mcp { binary } => mcp::serve(&root, binary),
        Cmd::MockAi { port } => kore_testkit::mock_ai::serve(port),
    }
}

fn do_action(a: &mut App, action: DoAction) -> Result<()> {
    match action {
        DoAction::Text => {
            for h in a.read_screen()? {
                let (rx, ry) = (h.x - a.frame.x, h.y - a.frame.y);
                println!("{:>5.0},{:>4.0} ({:.3},{:.3})  {}", rx, ry, rx / a.frame.w, ry / a.frame.h, h.text);
            }
        }
        DoAction::Screenshot { out } => a.screenshot(&out)?,
        DoAction::Click { text, nth, double } => {
            let q = TextQuery { text, nth, ..Default::default() };
            let h = a.click_text(&q, Duration::from_secs(5), if double { 2 } else { 1 })?;
            println!("clicked {:?} at {:?}", h.text, h.center());
        }
        DoAction::ClickAt { x, y } => a.click_at(x, y)?,
        DoAction::Key { chord } => a.key(&chord)?,
        DoAction::Type { text } => a.type_text(&text)?,
        DoAction::Scroll { x, y, dy } => {
            let f = a.frame;
            a.scroll_at(f.x + x * f.w, f.y + y * f.h, dy)?;
        }
        DoAction::Wait { text, timeout } => {
            let h = a.wait_for(&text.as_str().into(), Duration::from_secs_f64(timeout))?;
            println!("visible {:?} at {:?}", h.text, h.center());
        }
        DoAction::Snapshot => println!("{}", serde_json::to_string_pretty(&a.automation().snapshot()?)?),
        DoAction::Frames { reset } => println!("{:?}", a.automation().frame_stats(reset)?),
    }
    Ok(())
}

fn run(root: &Path, binary: Option<PathBuf>, layer: Layer, list: &[scenario::Scenario], out: &Path, keep_open: bool) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let home = out.join("home");
    let socket = driver::socket_path("run");
    let cfg = |extra: &[(String, String)]| -> Result<LaunchConfig> {
        let mut c = launch_config(root, binary.clone(), home.clone(), layer, socket.clone(), out.join("app.log"))?;
        // `${HARNESS_DIR}` in env values expands to the absolute `harness/` directory.
        let hdir = root.join("harness").to_string_lossy().into_owned();
        c.extra_env = extra.iter().map(|(k, v)| (k.clone(), v.replace("${HARNESS_DIR}", &hdir))).collect();
        Ok(c)
    };
    let scratch = out.join("scratch");
    let mut app: Option<App> = None;
    let mut results = Vec::new();
    for sc in list {
        eprintln!("▶ {}", sc.name);
        let restart = app.is_none() || sc.fresh_app || !sc.env.is_empty() || !app.as_mut().unwrap().is_running();
        if restart {
            if let Some(a) = app.take() {
                a.quit()?;
            }
            app = Some(App::launch(&cfg(&sc.env)?, &scratch)?);
        }
        if layer == Layer::Os {
            os::guard::scenario_started(app.as_ref().map(|a| a.pid));
        }
        let a = app.as_mut().unwrap();
        let mut runner = scenario::Runner { app: a, out_dir: out.to_path_buf(), kubeconfig: root.join("harness/.kube/config") };
        let r = runner.run(sc);
        if layer == Layer::Os {
            os::input::release_all();
        }
        eprintln!("{} {} ({} ms)\n", if r.ok { "✓ passed" } else { "✗ FAILED" }, r.name, r.millis);
        // A scenario that changed env-specific setup, or failed part-way (leaving dialogs or
        // half-finished state behind), taints the app; restart for the next one.
        if !sc.env.is_empty() || !r.ok {
            if let Some(a) = app.take() {
                a.quit()?;
            }
        }
        results.push(r);
    }
    if let Some(a) = app.take() {
        if keep_open {
            println!("app left running: pid {}", a.pid);
            a.detach();
        } else {
            a.quit()?;
        }
    }
    report::write(out, &results)?;
    let failed = results.iter().filter(|r| !r.ok).count();
    println!("{} scenarios, {} failed — report: {}", results.len(), failed, out.join("report.html").display());
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
