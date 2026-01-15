# Kore (Tauri v2 + Rust + React)

Kore is a macOS desktop app (Tauri v2) that offers a terminal-inspired, k9s-style UI for Kubernetes clusters using Rust (kube-rs) and React (Vite + Tailwind + shadcn/ui).

## Prerequisites
- macOS 14+ (Tauri target in config)
- Rust toolchain (stable) + `cargo`
- Node.js 18+ and `npm`
- Tauri tooling deps for macOS (Xcode CLT, Rust, etc.): see https://tauri.app/start/prerequisites/
- Access to a valid `~/.kube/config` with at least one context

## Install dependencies
```bash
cd /Users/eladbash/git/kore
npm install
```

## Run in development
```bash
# launches Vite + Tauri dev window
npm run tauri:dev
```

## Build release binary
```bash
npm run tauri:build
```

## Useful scripts
- `npm run dev` – Vite only (frontend)
- `npm run tauri:dev` – full app (recommended)
- `npm run tauri:build` – production bundle

## Notes
- The app reads your active kubeconfig and can switch contexts via the command palette (cmd+k).
- Watches stream resources via Tauri events; ensure your cluster is reachable.
- Logs and deletes use the selected namespace/kind; be cautious when deleting resources.


