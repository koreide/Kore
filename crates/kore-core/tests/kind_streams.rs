//! Logs, multi-pod logs, exec, metrics and port-forward against kind.
mod common;

use common::*;
use kore_core::{CoreConfig, LogRequest};
use std::io::{Read, Write};
use std::time::Duration;

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn logs_fetch_and_stream() {
    require_kind!();
    let core = core();
    let logger = running_pod(DEMO_NS, "app=logger");
    block_on(async {
        let text = core
            .fetch_logs(LogRequest {
                namespace: DEMO_NS.into(),
                pod: logger.clone(),
                container: None,
                tail_lines: Some(5),
                previous: None,
            })
            .await
            .unwrap();
        assert!(text.contains("KORE-LOG"), "{text}");

        // Single container: plain lines, append=true
        let mut sub = core
            .start_pod_logs_stream(DEMO_NS.into(), logger.clone(), None, None)
            .await
            .unwrap();
        let line = recv_until(&core, &mut sub, Duration::from_secs(20), |p| {
            p.logs.as_deref().is_some_and(|l| l.contains("KORE-LOG"))
        })
        .await
        .expect("no logger line");
        assert_eq!(line.append, Some(true));
        assert!(line.logs.unwrap().ends_with('\n'));

        // Multi-container: lines prefixed with [container]
        let mut multi = core
            .start_pod_logs_stream(DEMO_NS.into(), "multi-container".into(), None, None)
            .await
            .unwrap();
        let mut seen_main = false;
        let mut seen_side = false;
        let _ = recv_until(&core, &mut multi, Duration::from_secs(20), |p| {
            let l = p.logs.clone().unwrap_or_default();
            seen_main |= l.starts_with("[main] MAIN-CONTAINER-LOG");
            seen_side |= l.starts_with("[sidecar] SIDECAR-CONTAINER-LOG");
            seen_main && seen_side
        })
        .await;
        assert!(
            seen_main && seen_side,
            "main={seen_main} sidecar={seen_side}"
        );

        // Both streams were live at the same time (no single global slot).
        assert!(!sub.is_cancelled());
        let again = recv_until(&core, &mut sub, Duration::from_secs(10), |p| {
            p.logs.is_some()
        })
        .await;
        assert!(again.is_some(), "first stream died when the second started");

        // previous=true on the crashlooping pod returns its last output
        let mut prev = core
            .start_pod_logs_stream(DEMO_NS.into(), "crashloop".into(), None, Some(true))
            .await
            .unwrap();
        let p = recv_until(&core, &mut prev, Duration::from_secs(20), |p| {
            p.logs.as_deref().is_some_and(|l| l.contains("KORE-CRASH"))
        })
        .await;
        assert!(p.is_some(), "no previous logs for crashloop");

        // previous=true on a never-restarted pod → info payload (no append)
        let mut none = core
            .start_pod_logs_stream(
                DEMO_NS.into(),
                logger.clone(),
                Some("log".into()),
                Some(true),
            )
            .await
            .unwrap();
        let info = recv_until(&core, &mut none, Duration::from_secs(20), |_| true)
            .await
            .expect("no payload");
        assert!(
            info.logs.unwrap().starts_with("No previous logs"),
            "unexpected"
        );
        assert_eq!(info.append, None);

        // stop_pod_logs_stream cancels every log stream
        core.stop_pod_logs_stream().await.unwrap();
        assert!(sub.is_cancelled() && multi.is_cancelled());
        let ended = timeout(
            &core,
            Duration::from_secs(5),
            Box::pin(async { while sub.recv().await.is_some() {} }),
        )
        .await;
        assert!(ended.is_some());
        // New streams still work after a stop
        let mut s2 = core
            .start_pod_logs_stream(DEMO_NS.into(), logger.clone(), None, None)
            .await
            .unwrap();
        assert!(recv_until(&core, &mut s2, Duration::from_secs(20), |p| p
            .logs
            .is_some())
        .await
        .is_some());
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn multi_pod_logs_by_selector() {
    require_kind!();
    let core = core();
    block_on(async {
        let mut stream = core
            .stream_multi_pod_logs(DEMO_NS.into(), "app=logger".into(), Some(5))
            .await
            .unwrap();
        assert_eq!(stream.pods.len(), 2);
        assert_ne!(stream.pods[0].color, stream.pods[1].color);
        let pods: Vec<String> = stream.pods.iter().map(|p| p.name.clone()).collect();
        let mut seen = std::collections::HashSet::new();
        let _ = recv_until(&core, &mut stream.lines, Duration::from_secs(30), |l| {
            assert!(l.line.contains("KORE-LOG"));
            assert!(pods.contains(&l.pod));
            seen.insert(l.pod.clone());
            seen.len() == 2
        })
        .await;
        assert_eq!(seen.len(), 2, "lines only from {seen:?}");

        // A single-pod stream does not kill the multi-pod stream.
        let _single = core
            .start_pod_logs_stream(DEMO_NS.into(), pods[0].clone(), None, None)
            .await
            .unwrap();
        let more = recv_until(&core, &mut stream.lines, Duration::from_secs(10), |_| true).await;
        assert!(more.is_some());

        // Selector matching nothing → validation error
        let err = core
            .stream_multi_pod_logs(DEMO_NS.into(), "app=does-not-exist".into(), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No pods found"));
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn exec_echo_resize_exit() {
    require_kind!();
    let core = core();
    block_on(async {
        let mut sess = core
            .exec_into_pod(
                DEMO_NS.into(),
                "multi-container".into(),
                Some("main".into()),
                None,
            )
            .await
            .unwrap();
        assert!(sess.session_id.starts_with("kore-demo/multi-container/"));
        let sid = sess.session_id.clone();

        let mut out = String::new();
        core.send_exec_input(sid.clone(), "echo kore-exec-$((40+2))\n".into())
            .await
            .unwrap();
        assert!(
            read_until(&core, &mut sess.stdout, &mut out, "kore-exec-42").await,
            "echo output missing: {out}"
        );

        // Deliberate Fix 7: resize reaches the TTY
        core.resize_exec(sid.clone(), 123, 45).await.unwrap();
        sleep(&core, Duration::from_millis(500)).await;
        core.send_exec_input(sid.clone(), "stty size\n".into())
            .await
            .unwrap();
        assert!(
            read_until(&core, &mut sess.stdout, &mut out, "45 123").await,
            "stty size did not reflect resize: {out}"
        );

        core.send_exec_input(sid.clone(), "exit\n".into())
            .await
            .unwrap();
        let exit = timeout(&core, Duration::from_secs(20), Box::pin(&mut sess.exit))
            .await
            .expect("no exit")
            .expect("exit sender dropped");
        assert_eq!(exit.reason.as_deref(), Some("completed"));
        // Session is removed after exit
        sleep(&core, Duration::from_millis(300)).await;
        assert!(core.send_exec_input(sid.clone(), "x".into()).await.is_err());

        // Dropping a session's stdout ends it (stop via drop)
        let sess2 = core
            .exec_into_pod(
                DEMO_NS.into(),
                "multi-container".into(),
                Some("sidecar".into()),
                None,
            )
            .await
            .unwrap();
        let sid2 = sess2.session_id.clone();
        let exit2 = sess2.exit;
        drop(sess2.stdout);
        let e = timeout(&core, Duration::from_secs(20), Box::pin(exit2)).await;
        assert!(e.is_some(), "session did not end after drop");
        assert!(
            core.stop_exec(sid2).await.is_err(),
            "session still registered"
        );

        // Bad shell → exit with error
        let sess3 = core
            .exec_into_pod(
                DEMO_NS.into(),
                "multi-container".into(),
                Some("main".into()),
                Some("/no/such/shell".into()),
            )
            .await
            .unwrap();
        let mut stdout3 = sess3.stdout;
        let _ = recv_until(&core, &mut stdout3, Duration::from_secs(10), |_| false).await;
        let e3 = timeout(&core, Duration::from_secs(20), Box::pin(sess3.exit)).await;
        assert!(matches!(e3, Some(Ok(_))), "no exit payload for bad shell");
    });
}

async fn read_until(
    core: &kore_core::Core,
    sub: &mut kore_core::Subscription<Vec<u8>>,
    out: &mut String,
    needle: &str,
) -> bool {
    if out.contains(needle) {
        return true;
    }
    recv_until(core, sub, Duration::from_secs(20), |b| {
        out.push_str(&String::from_utf8_lossy(b));
        out.contains(needle)
    })
    .await
    .is_some()
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn pod_metrics() {
    require_kind!();
    let core = core();
    let logger = running_pod(DEMO_NS, "app=logger");
    block_on(async {
        let mut last = None;
        for _ in 0..30 {
            match core.get_pod_metrics(DEMO_NS.into(), logger.clone()).await {
                Ok(m) => {
                    last = Some(m);
                    break;
                }
                Err(_) => sleep(&core, Duration::from_secs(2)).await,
            }
        }
        let m = last.expect("metrics never became available");
        assert!(m["containers"][0]["usage"]["cpu"].is_string(), "{m}");
        assert!(m["containers"][0]["usage"]["memory"].is_string(), "{m}");
    });
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http_get(port: u16) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn port_forward_http_get() {
    require_kind!();
    let core = core();
    let web = running_pod(DEMO_NS, "app=web");
    let port = free_port();
    let res = block_on(core.start_port_forward(DEMO_NS.into(), web.clone(), port, 80)).unwrap();
    assert_eq!(res["status"], "active");
    assert_eq!(res["localPort"], port);
    let body = http_get(port);
    assert!(body.starts_with("HTTP/1.1 200"), "{body}");
    assert!(body.contains("nginx"), "{body}");
    // Second forward on the same port fails
    let err = block_on(core.start_port_forward(DEMO_NS.into(), web.clone(), port, 80)).unwrap_err();
    assert!(err.to_string().contains("already"), "{err}");
    block_on(core.stop_port_forward(DEMO_NS.into(), web.clone(), port, 80)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "listener still open"
    );
}

/// Deliberate Fix 6: the forward targets Kore's selected context even though
/// the kubeconfig's current-context is the other cluster.
#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn port_forward_uses_selected_context() {
    require_kind!();
    let dir = tempfile::tempdir().unwrap();
    let core = kore_core::Core::with_config(CoreConfig {
        kubeconfig_paths: vec![kubeconfig()],
        initial_context: Some(CTX2.into()),
        data_dir: Some(dir.path().join("data")),
        favorites_dir: Some(dir.path().join("kore")),
        ..Default::default()
    })
    .unwrap();
    let (ok, pod) = kubectl_ctx(
        CTX2,
        &[
            "-n",
            "kore-remote",
            "get",
            "pods",
            "-l",
            "app=remote-web",
            "-o",
            "jsonpath={.items[0].metadata.name}",
        ],
    );
    assert!(ok && !pod.is_empty(), "{pod}");
    let port = free_port();
    block_on(core.start_port_forward("kore-remote".into(), pod.clone(), port, 80)).unwrap();
    let body = http_get(port);
    assert!(body.starts_with("HTTP/1.1 200"), "{body}");
    block_on(core.stop_port_forward("kore-remote".into(), pod, port, 80)).unwrap();
}
