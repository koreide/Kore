import { useEffect, useRef, useState } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { execIntoPod, sendExecInput, stopExec } from "@/lib/api";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, Loader2 } from "lucide-react";

interface ExecTerminalProps {
  namespace: string;
  podName: string;
  container?: string;
}

export function ExecTerminal({ namespace, podName, container }: ExecTerminalProps) {
  const termRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const [status, setStatus] = useState<"connecting" | "connected" | "error" | "closed">(
    "connecting",
  );
  const [error, setError] = useState<string | null>(null);
  const [shell, setShell] = useState("/bin/sh");

  useEffect(() => {
    if (!termRef.current) return;

    const term = new XTerm({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "SFMono-Regular, SF Mono, Menlo, Consolas, monospace",
      theme: {
        background: "#0b1221",
        foreground: "#e2e8f0",
        cursor: "#58d0ff",
        selectionBackground: "#58d0ff33",
        black: "#1e293b",
        red: "#f87171",
        green: "#4ade80",
        yellow: "#fbbf24",
        blue: "#60a5fa",
        magenta: "#c084fc",
        cyan: "#22d3ee",
        white: "#e2e8f0",
        brightBlack: "#475569",
        brightRed: "#fca5a5",
        brightGreen: "#86efac",
        brightYellow: "#fde68a",
        brightBlue: "#93c5fd",
        brightMagenta: "#d8b4fe",
        brightCyan: "#67e8f9",
        brightWhite: "#f8fafc",
      },
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);
    term.open(termRef.current);
    fitAddon.fit();

    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    // Handle user input
    term.onData((data) => {
      if (sessionIdRef.current) {
        sendExecInput(sessionIdRef.current, data).catch(() => {});
      }
    });

    // Handle resize
    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
    });
    resizeObserver.observe(termRef.current);

    // Start exec session
    let unlistenStdout: (() => void) | null = null;
    let unlistenExit: (() => void) | null = null;
    let isMounted = true;

    const startSession = async () => {
      try {
        const sessionId = await execIntoPod(namespace, podName, container, shell);
        if (!isMounted) {
          stopExec(sessionId).catch(() => {});
          return;
        }

        sessionIdRef.current = sessionId;
        setStatus("connected");

        // Listen for stdout
        const stdoutEvent = `exec-stdout://${sessionId}`;
        unlistenStdout = await listen<{ data: string }>(stdoutEvent, (event) => {
          if (!isMounted) return;
          // Decode base64
          try {
            const bytes = atob(event.payload.data);
            term.write(bytes);
          } catch {
            term.write(event.payload.data);
          }
        });

        // Listen for exit
        const exitEvent = `exec-exit://${sessionId}`;
        unlistenExit = await listen<{ reason?: string; error?: string }>(exitEvent, (event) => {
          if (!isMounted) return;
          if (event.payload.error) {
            setError(event.payload.error);
            setStatus("error");
            term.write(`\r\n\x1b[31m${event.payload.error}\x1b[0m\r\n`);
          } else {
            setStatus("closed");
            term.write("\r\n\x1b[33mSession ended.\x1b[0m\r\n");
          }
        });
      } catch (err) {
        if (!isMounted) return;
        setError(String(err));
        setStatus("error");
      }
    };

    startSession();

    return () => {
      isMounted = false;
      if (unlistenStdout) unlistenStdout();
      if (unlistenExit) unlistenExit();
      if (sessionIdRef.current) {
        stopExec(sessionIdRef.current).catch(() => {});
      }
      resizeObserver.disconnect();
      term.dispose();
    };
  }, [namespace, podName, container, shell]);

  return (
    <div className="h-full flex flex-col">
      {/* Shell selector */}
      <div className="flex items-center gap-2 px-4 py-2 border-b border-slate-800/50 bg-surface/30">
        <span className="text-[10px] uppercase text-slate-500">Shell:</span>
        {["/bin/sh", "/bin/bash"].map((s) => (
          <button
            key={s}
            onClick={() => setShell(s)}
            className={`px-2 py-0.5 rounded text-xs transition ${
              shell === s
                ? "bg-accent/10 text-accent border border-accent/30"
                : "text-slate-400 hover:text-slate-200 border border-slate-800"
            }`}
          >
            {s.split("/").pop()}
          </button>
        ))}
        {status === "connecting" && (
          <span className="flex items-center gap-1 text-xs text-amber-400 ml-auto">
            <Loader2 className="w-3 h-3 animate-spin" />
            Connecting...
          </span>
        )}
        {status === "connected" && (
          <span className="text-xs text-emerald-400 ml-auto">Connected</span>
        )}
        {status === "error" && (
          <span className="flex items-center gap-1 text-xs text-red-400 ml-auto">
            <AlertCircle className="w-3 h-3" />
            {error || "Error"}
          </span>
        )}
        {status === "closed" && (
          <span className="text-xs text-slate-400 ml-auto">Session ended</span>
        )}
      </div>
      <div ref={termRef} className="flex-1 p-1" />
    </div>
  );
}
