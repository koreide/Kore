//! Shared helpers for kind integration tests.
//!
//! Tests are `#[ignore]`d and additionally skip unless `KORE_KIND=1`:
//! `KORE_KIND=1 cargo test -p kore-core -- --include-ignored --test-threads=1`
//!
//! Every test drives `Core` from `futures::executor::block_on` (a non-tokio
//! executor) to prove the futures/receivers work outside the core runtime.
#![allow(dead_code)]

use futures::future::{select, Either};
use kore_core::{Core, CoreConfig, Subscription};
use std::future::Future;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

pub const CTX: &str = "kind-kore-harness";
pub const CTX2: &str = "kind-kore-harness-2";
pub const DEMO_NS: &str = "kore-demo";

pub fn enabled() -> bool {
    std::env::var("KORE_KIND")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Skip (return early) unless KORE_KIND=1.
#[macro_export]
macro_rules! require_kind {
    () => {
        if !common::enabled() {
            eprintln!("skipping: set KORE_KIND=1 to run kind integration tests");
            return;
        }
    };
}

pub fn kubeconfig() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../harness/.kube/config")
        .canonicalize()
        .expect("harness/.kube/config missing — run `kore-harness cluster up`")
}

pub struct TestCore {
    pub core: Core,
    pub dir: tempfile::TempDir,
}

impl std::ops::Deref for TestCore {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.core
    }
}

pub fn core() -> TestCore {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_config(CoreConfig {
        kubeconfig_paths: vec![kubeconfig()],
        initial_context: Some(CTX.into()),
        data_dir: Some(dir.path().join("data")),
        favorites_dir: Some(dir.path().join("kore")),
        ..Default::default()
    })
    .unwrap();
    TestCore { core, dir }
}

pub fn block_on<F: Future>(f: F) -> F::Output {
    futures::executor::block_on(f)
}

/// A timer usable from any executor (spawned on the core runtime).
pub async fn sleep(core: &Core, d: Duration) {
    let _ = core.spawn(async move { tokio::time::sleep(d).await }).await;
}

/// Await `fut` with a timeout (timer runs on the core runtime).
pub async fn timeout<F: Future + Unpin>(core: &Core, d: Duration, fut: F) -> Option<F::Output> {
    let timer = Box::pin(sleep(core, d));
    match select(fut, timer).await {
        Either::Left((v, _)) => Some(v),
        Either::Right(_) => None,
    }
}

/// Receive items until `pred` matches one (returns it) or the timeout hits.
pub async fn recv_until<T, P: FnMut(&T) -> bool>(
    core: &Core,
    sub: &mut Subscription<T>,
    d: Duration,
    mut pred: P,
) -> Option<T> {
    let deadline = std::time::Instant::now() + d;
    loop {
        let left = deadline.checked_duration_since(std::time::Instant::now())?;
        match timeout(core, left, Box::pin(sub.recv())).await {
            Some(Some(v)) => {
                if pred(&v) {
                    return Some(v);
                }
            }
            Some(None) | None => return None,
        }
    }
}

pub fn kubectl(args: &[&str]) -> (bool, String) {
    kubectl_ctx(CTX, args)
}

pub fn kubectl_ctx(ctx: &str, args: &[&str]) -> (bool, String) {
    let out = Command::new("kubectl")
        .env("KUBECONFIG", kubeconfig())
        .arg("--context")
        .arg(ctx)
        .args(args)
        .output()
        .expect("kubectl not on PATH");
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), s)
}

pub fn kubectl_apply(yaml: &str) {
    use std::io::Write;
    let mut child = Command::new("kubectl")
        .env("KUBECONFIG", kubeconfig())
        .args(["--context", CTX, "apply", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(yaml.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "kubectl apply failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A throwaway namespace, deleted on drop (restores cluster state).
pub struct TempNamespace {
    pub name: String,
}

impl TempNamespace {
    pub fn new(prefix: &str) -> Self {
        let name = format!("{prefix}-{}", &uuid_like()[..8]);
        let (ok, out) = kubectl(&["create", "namespace", &name]);
        assert!(ok, "create namespace failed: {out}");
        Self { name }
    }
}

impl Drop for TempNamespace {
    fn drop(&mut self) {
        let _ = kubectl(&["delete", "namespace", &self.name, "--wait=false"]);
    }
}

pub fn uuid_like() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Name of the first Running pod matching `selector` in `ns`.
pub fn running_pod(ns: &str, selector: &str) -> String {
    let (ok, out) = kubectl(&[
        "-n",
        ns,
        "get",
        "pods",
        "-l",
        selector,
        "--field-selector=status.phase=Running",
        "-o",
        "jsonpath={.items[0].metadata.name}",
    ]);
    assert!(
        ok && !out.trim().is_empty(),
        "no running pod for {selector}: {out}"
    );
    out.trim().to_string()
}

pub fn names(items: &[serde_json::Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| i.pointer("/metadata/name").and_then(|v| v.as_str()))
        .map(str::to_string)
        .collect()
}
