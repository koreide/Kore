# Changelog

All notable changes to Kore. Versions follow [Semantic Versioning](https://semver.org/);
release builds and downloads are on the [GitHub releases page](https://github.com/koreide/Kore/releases).

## [0.2.0] - 2026-10-17

Kore is now a native application: a single Rust binary with a GPU-rendered UI (GPUI),
replacing the Tauri + React app. It looks and behaves the same, starts faster and uses
much less memory (~120 MB instead of a WebView process tree).

### Changed
- **Native rewrite.** The UI is rebuilt in Rust on GPUI (`crates/kore-app`) on top of a
  UI-independent backend library (`crates/kore-core`, kube-rs). Every view of the previous
  app is ported with the same look, behavior and keyboard shortcuts. The legacy `src/`
  (React) and `src-tauri/` trees and the npm/Vite toolchain are removed.
- **Packaging and releases** for all three platforms from one workflow:
  - macOS DMGs (Apple Silicon and Intel), ad-hoc signed and not notarized (the installer and the in-app updater clear the quarantine flag);
  - a Linux tarball with `install.sh`;
  - a Windows installer (per-user, NSIS) and a portable zip;
  - all published straight to the GitHub release, each platform as soon as it is built, followed by `SHA256SUMS`.
- **In-app updates.** On macOS the app updates itself from the DMG. On Linux and Windows the
  update banner opens the release page instead of attempting an install. Version comparison is
  numeric, so an older release is never offered as an update.
- **Resource lists** page through the API (500 per page) instead of stopping at 500 objects.
- **Pod STATUS** shows what `kubectl` shows (Running, CrashLoopBackOff, Completed,
  Init:0/1, Error, Terminating…) instead of Ready/NotReady. RESTARTS sums all containers.
- **Context switching** reports whether the new cluster is reachable. A per-context default
  namespace from Settings is applied on switch and at startup. With no favorites, the app
  starts in the kubeconfig's `current-context`.

### Added
- **Command palette:** status quick filters (`:running`, `:failed`, `:pending`) with a removable
  "Status: …" chip, a visible keyboard selection, styled group headings, a live Recent list and a
  Network Policies entry.
- **`d` shortcut** to delete the highlighted table row.
- **RBAC "forbidden" banner** in Network Policies, Custom Resources, Graph, Helm and RBAC, with
  the closest matching rules.
- **Helm:** release notes in the Notes tab, and Helm's own timestamps formatted like the history.
- **Graph:** hover tooltips; clicking a ReplicaSet opens its owning Deployment.
- **Dashboard:** live node CPU/memory usage from metrics-server and human-readable ages.
- **Events history** retention is configurable in Settings and applied to the event store.

### Fixed
- **Details views.** The status badge in a pod's header now updates live. Cards are no longer
  clipped at the bottom. Long wrapped log lines no longer paint over the next line. Timestamps in
  the Describe JSON are no longer altered. Plain byte counts in memory limits are parsed correctly.
  The Events Live/History toggle is always shown. The pod Node field shows the node name.
- **Deployment logs** use the Deployment's real label selector and stream every container.
- **Cluster-scoped custom resources** open and delete correctly. Cluster-scoped kinds (nodes,
  namespaces) delete without a namespace.
- **Confirm dialogs** have correct titles ("Delete Ingress") and cancel with Esc or a backdrop click.
- **Focus.** The AI Chat and Network Policies inputs no longer steal focus from a field you
  clicked right after opening the view.
- **Debug builds.** The Network Policies view and several buttons (Restart, Debug, YAML editor)
  no longer crash debug builds.
- **Release notes for earlier merges.** Also includes the changes merged after 0.1.2: better
  error handling and tests (#12), the left panel scroll fix (#13), and the AI settings and
  Settings page layout fix (#14).

### Testing
Testing is now three layers, cheapest first (see "Testing layers" in `CLAUDE.md`):
- **Layer 1 (`just test-unit`, CI):** 124 headless GPUI tests drive the real UI against an
  in-memory Kubernetes API (`crates/kore-testkit`), next to the unit tests. They need no cluster,
  network, keychain or window. `kore-core` takes every outside dependency through `CoreConfig`:
  cluster connector, secret store, update URL, tool directory and AI endpoints.
- **Layer 2 (`just test-app`):** the real app, built with the `test-harness` feature, runs in the
  background against kind clusters. It is driven through an in-app control server on a Unix
  socket and screenshotted off-screen, so it never touches the mouse or keyboard. Release builds
  never contain the control server; CI checks this. Scenarios can target elements by id
  (`click: {id: …}`, `element:`, `element_gone:`). The same control is available to agents as an
  MCP server (`kore-harness mcp`, `.mcp.json`).
- **Layer 3 (`just test-os <name>`):** real OS input, only for what Layers 1–2 can't reach. It runs
  only named scenarios, and always releases input and stops the app on completion, Ctrl-C, panic
  or a per-scenario timeout.

## [0.1.2] - 2026-03-13
### Fixed
- Logs of pods with multiple containers (#7).

## [0.1.1] - 2026-03-10
### Fixed
- Unreachable contexts no longer block startup; installation improvements (#6).

## [0.1.0] - 2026-03-10
### Added
- Debug containers (#3).
- RBAC simulator (#4).
- AI-assisted network policy view and in-app update checks (#5).

## [0.0.11] - 2026-03-02
## [0.0.10] - 2026-03-01
Early releases.

[0.2.0]: https://github.com/koreide/Kore/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/koreide/Kore/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/koreide/Kore/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/koreide/Kore/compare/v0.0.11...v0.1.0
[0.0.11]: https://github.com/koreide/Kore/releases/tag/v0.0.11
[0.0.10]: https://github.com/koreide/Kore/releases/tag/v0.0.10
