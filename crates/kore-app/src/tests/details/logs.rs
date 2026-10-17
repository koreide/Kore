//! Pod Logs tab: streaming, search, copy, download, previous, containers.

use super::helpers::{DetailsExt, NS};
use crate::testing::Kore;
use crate::views::pod_logs::DownloadDir;
use gpui_kit::TestAppContext;

const LOGGER: &str = "logger-684d79cb86-26m9x";

/// Scenario 20-details-logs-stream: a single-container logger pod streams
/// numbered lines (followed: appended lines show up) with no container
/// select; ⌘F counts and steps through matches (Enter / Shift+Enter) and its X
/// closes it on the pod; Copy puts the log on the clipboard; Download writes
/// the file and toasts; Auto-scroll toggles; Previous restarts the stream with
/// `previous=true`. (Level colors are Layer 2.)
#[gpui_kit::test]
fn pod_logs_stream_search_copy_download_and_previous(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    // Never the real ~/Downloads: the sandbox's temp dir.
    let downloads = k.env.dir.path().join("downloads");
    let dir = downloads.clone();
    k.cx.update(|_, cx| cx.set_global(DownloadDir(dir)));
    k.open_pod(LOGGER);
    k.wait_row("logs.row.", "level=INFO");
    k.wait_row("logs.row.", "level=ERROR");
    for id in ["logs.previous", "logs.autoscroll", "logs.copy", "logs.download"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    assert!(k.element("logs.container").is_none(), "single container: no select");
    k.wait_row("logs.row.", "line=153143 level=INFO");
    k.cluster.append_log(NS, LOGGER, "log", &format!("KORE-LOG {LOGGER} line=999999 level=ERROR"));
    k.wait_row("logs.row.", "line=999999");

    // Search
    k.open_search();
    k.type_text("level=ERROR");
    k.wait_label("searchbar.count", "1/N", |l| l.starts_with("1/"));
    let n: usize = k.label("searchbar.count").unwrap()[2..].parse().unwrap();
    assert_eq!(n, 9, "8 fixture ERROR lines + the appended one");
    k.keys("enter");
    k.wait_label("searchbar.count", "2/N", |l| l == format!("2/{n}"));
    k.keys("shift-enter");
    k.wait_label("searchbar.count", "1/N", |l| l == format!("1/{n}"));
    k.click("searchbar.close");
    k.wait_gone("searchbar");
    assert!(k.element("logs.previous").is_some() && k.element("logs.autoscroll").is_some());
    assert_eq!(k.state()["view"], "details");

    // Copy / Download
    k.click("logs.copy");
    k.wait_toast("copied", |l| l == "Copied to clipboard");
    let clip = k.cx.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default();
    assert_eq!(clip.lines().count(), 41);
    assert!(clip.contains("line=999999 level=ERROR"));
    k.click("logs.download");
    k.wait_toast("downloaded", |l| l == "Logs downloaded");
    let files: Vec<_> = std::fs::read_dir(&downloads).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().to_string_lossy().to_string();
    assert!(name.starts_with(&format!("{LOGGER}-")) && name.ends_with(".log"), "{name}");
    assert_eq!(std::fs::read_to_string(files[0].path()).unwrap(), clip);

    // Auto-scroll toggles off and back on.
    k.click("logs.autoscroll");
    k.wait_until("autoscroll off", |k| k.element("logs.autoscroll").is_some_and(|e| !e.selected));
    k.click("logs.autoscroll");
    k.wait_until("autoscroll on", |k| k.element("logs.autoscroll").is_some_and(|e| e.selected));

    // Previous: the stream restarts with previous=true (and without follow).
    k.cluster.clear_requests();
    k.click("logs.previous");
    k.wait_until("previous request", |k| {
        k.cluster.requests().iter().any(|r| r.path.ends_with(&format!("/pods/{LOGGER}/log")) && r.query.get("previous").map(String::as_str) == Some("true"))
    });
    assert!(k.element("logs.previous").unwrap().selected);
    let req = k.cluster.requests().into_iter().find(|r| r.query.contains_key("previous")).unwrap();
    assert_ne!(req.query.get("follow").map(String::as_str), Some("true"));
    k.click("logs.previous");
    k.wait_until("previous off", |k| k.element("logs.previous").is_some_and(|e| !e.selected));
    k.wait_row("logs.row.", "KORE-LOG logger-");
}

/// The whole log buffer, through Copy.
fn copied_logs(k: &mut Kore) -> String {
    k.click("logs.copy");
    k.cx.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default()
}

/// Scenario 20-details-logs-containers: on the multi-container pod "All
/// containers" interleaves `[main]` and `[sidecar]` prefixed lines; choosing
/// sidecar or main shows only that container's lines without prefix; All
/// restores both.
#[gpui_kit::test]
fn container_select_filters_pod_logs(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open_pod("multi-container");
    k.wait_label("logs.container", "All containers", |l| l == "All containers");
    k.wait_label("logs.list", "80 lines", |l| l == "81 rows");
    let all = copied_logs(&mut k);
    assert_eq!(all.lines().filter(|l| *l == "[main] MAIN-CONTAINER-LOG").count(), 40);
    assert_eq!(all.lines().filter(|l| *l == "[sidecar] SIDECAR-CONTAINER-LOG").count(), 40);

    k.click("logs.container");
    k.click("logs.container.option.sidecar");
    k.wait_label("logs.container", "sidecar", |l| l == "sidecar");
    k.wait_until("only sidecar lines", |k| {
        let rows = k.labels("logs.row.");
        !rows.is_empty() && rows.iter().all(|l| l == "SIDECAR-CONTAINER-LOG" || l.is_empty())
    });
    assert!(copied_logs(&mut k).lines().all(|l| l == "SIDECAR-CONTAINER-LOG"));

    k.click("logs.container");
    k.click("logs.container.option.main");
    k.wait_label("logs.container", "main", |l| l == "main");
    k.wait_until("only main lines", |k| {
        let rows = k.labels("logs.row.");
        !rows.is_empty() && rows.iter().all(|l| l == "MAIN-CONTAINER-LOG" || l.is_empty())
    });
    assert!(copied_logs(&mut k).lines().all(|l| l == "MAIN-CONTAINER-LOG"));

    k.click("logs.container");
    k.click("logs.container.option.");
    k.wait_label("logs.container", "All containers", |l| l == "All containers");
    // Both containers' 40 lines, prefixed (stream arrival order varies).
    k.wait_label("logs.list", "80 lines", |l| l == "81 rows");
    let all = copied_logs(&mut k);
    assert_eq!(all.lines().filter(|l| *l == "[main] MAIN-CONTAINER-LOG").count(), 40);
    assert_eq!(all.lines().filter(|l| *l == "[sidecar] SIDECAR-CONTAINER-LOG").count(), 40);
}
