import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Plug, X, ExternalLink, AlertCircle, Loader2 } from "lucide-react";
import { startPortForward, stopPortForward } from "@/lib/api";
import { formatError } from "@/lib/errors";
import type { PortForwardInfo } from "@/lib/types";
import { useToast } from "./toast";

interface PortForwardingProps {
  namespace: string;
  podName: string;
}

export function PortForwarding({ namespace, podName }: PortForwardingProps) {
  const [portForwards, setPortForwards] = useState<PortForwardInfo[]>([]);
  const [localPortInput, setLocalPortInput] = useState<string>("8080");
  const [podPortInput, setPodPortInput] = useState<string>("80");
  const [isStarting, setIsStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toast = useToast();

  const getRandomPort = (): number => {
    return Math.floor(Math.random() * (65535 - 49152 + 1)) + 49152;
  };

  const isPortInUse = (port: number): boolean => {
    return portForwards.some((pf) => pf.localPort === port && pf.status === "active");
  };

  const handleStartPortForward = async () => {
    setError(null);

    const podPort = parseInt(podPortInput, 10);
    if (isNaN(podPort) || podPort < 1 || podPort > 65535) {
      setError("Pod port must be a valid number between 1 and 65535");
      return;
    }

    let localPort: number;
    const inputLocalPort = parseInt(localPortInput, 10);

    if (!localPortInput || localPortInput.trim() === "" || inputLocalPort === 0) {
      let attempts = 0;
      do {
        localPort = getRandomPort();
        attempts++;
        if (attempts > 100) {
          setError("Unable to find an available port. Please specify a port manually.");
          return;
        }
      } while (isPortInUse(localPort));
    } else {
      if (isNaN(inputLocalPort) || inputLocalPort < 1 || inputLocalPort > 65535) {
        setError("Local port must be a valid number between 1 and 65535");
        return;
      }
      if (isPortInUse(inputLocalPort)) {
        setError(`Port ${inputLocalPort} is already in use by another port forward`);
        return;
      }
      localPort = inputLocalPort;
    }

    setIsStarting(true);

    const forwardId = `${namespace}/${podName}/${localPort}/${podPort}`;
    const newForward: PortForwardInfo = {
      id: forwardId,
      localPort,
      podPort,
      status: "connecting",
      statusMessage: "Connecting...",
      localAddress: `http://127.0.0.1:${localPort}`,
    };

    setPortForwards((prev) => [...prev, newForward]);

    try {
      const result = await startPortForward({
        namespace,
        podName,
        localPort,
        podPort,
      });

      setPortForwards((prev) =>
        prev.map((pf) =>
          pf.id === forwardId
            ? {
                ...pf,
                status: "active" as const,
                statusMessage: "Connected",
                localPort: result.localPort || localPort,
                localAddress: `http://127.0.0.1:${result.localPort || localPort}`,
              }
            : pf,
        ),
      );

      toast(`Port forward active on :${result.localPort || localPort}`, "success");
      setLocalPortInput("8080");
      setPodPortInput("80");
    } catch (err: unknown) {
      const errorMessage = formatError(err);

      setPortForwards((prev) =>
        prev.map((pf) =>
          pf.id === forwardId
            ? {
                ...pf,
                status: "error" as const,
                statusMessage: errorMessage,
              }
            : pf,
        ),
      );
      setError(errorMessage);
    } finally {
      setIsStarting(false);
    }
  };

  const handleStopPortForward = async (forwardId: string) => {
    const forward = portForwards.find((pf) => pf.id === forwardId);
    if (!forward) return;

    try {
      await stopPortForward({
        namespace,
        podName,
        localPort: forward.localPort,
        podPort: forward.podPort,
      });
      setPortForwards((prev) => prev.filter((pf) => pf.id !== forwardId));
    } catch (err: unknown) {
      setError(formatError(err));
    }
  };

  const handleOpenInBrowser = async (url: string) => {
    try {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open(url);
    } catch {
      window.open(url, "_blank");
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, x: -20 }}
      animate={{ opacity: 1, x: 0 }}
      className="w-80 border-l border-slate-800 bg-surface/50 flex flex-col"
    >
      {/* Header */}
      <div className="p-4 border-b border-slate-800">
        <div className="flex items-center gap-2 mb-2">
          <Plug className="w-5 h-5 text-accent" />
          <h3 className="text-lg font-semibold text-slate-100">Port Forwarding</h3>
        </div>
        <p className="text-xs text-slate-400">Forward local ports to pod ports</p>
      </div>

      {/* Configuration Form */}
      <div className="p-4 border-b border-slate-800 space-y-4">
        <div>
          <label className="block text-xs text-slate-400 mb-1.5">Pod Port</label>
          <input
            type="number"
            value={podPortInput}
            onChange={(e) => {
              setPodPortInput(e.target.value);
              setError(null);
            }}
            placeholder="80"
            min="1"
            max="65535"
            className="w-full px-3 py-2 bg-background border border-slate-800 rounded text-sm text-slate-100 placeholder-slate-500 focus:outline-none focus:border-accent transition"
            disabled={isStarting}
          />
        </div>
        <div>
          <label className="block text-xs text-slate-400 mb-1.5">
            Local Port <span className="text-slate-500">(leave blank for auto)</span>
          </label>
          <input
            type="number"
            value={localPortInput}
            onChange={(e) => {
              setLocalPortInput(e.target.value);
              setError(null);
            }}
            placeholder="8080"
            min="0"
            max="65535"
            className="w-full px-3 py-2 bg-background border border-slate-800 rounded text-sm text-slate-100 placeholder-slate-500 focus:outline-none focus:border-accent transition"
            disabled={isStarting}
          />
        </div>
        {error && (
          <motion.div
            initial={{ opacity: 0, y: -10 }}
            animate={{ opacity: 1, y: 0 }}
            className="flex items-center gap-2 p-2 bg-red-500/10 border border-red-500/50 rounded text-xs text-red-400"
          >
            <AlertCircle className="w-4 h-4 flex-shrink-0" />
            <span>{error}</span>
          </motion.div>
        )}
        <button
          onClick={handleStartPortForward}
          disabled={isStarting || !podPortInput}
          className="w-full px-4 py-2 bg-accent/20 border border-accent rounded text-sm text-accent hover:bg-accent/30 transition disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
        >
          {isStarting ? (
            <>
              <Loader2 className="w-4 h-4 animate-spin" />
              Starting...
            </>
          ) : (
            <>
              <Plug className="w-4 h-4" />
              Start Forward
            </>
          )}
        </button>
      </div>

      {/* Active Port Forwards List */}
      <div className="flex-1 overflow-y-auto p-4 space-y-3">
        {portForwards.length === 0 ? (
          <div className="text-center py-8 text-slate-500 text-sm">No active port forwards</div>
        ) : (
          <AnimatePresence>
            {portForwards.map((forward) => (
              <motion.div
                key={forward.id}
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
                className={`p-3 rounded border ${
                  forward.status === "active"
                    ? "bg-green-500/10 border-green-500/50"
                    : forward.status === "connecting"
                      ? "bg-yellow-500/10 border-yellow-500/50"
                      : "bg-red-500/10 border-red-500/50"
                }`}
              >
                {/* Status Indicator */}
                <div className="flex items-start justify-between mb-2">
                  <div className="flex items-center gap-2">
                    {forward.status === "active" && (
                      <div className="relative">
                        <div className="w-2 h-2 bg-green-500 rounded-full" />
                        <div className="absolute inset-0 w-2 h-2 bg-green-500 rounded-full animate-ping opacity-75" />
                      </div>
                    )}
                    {forward.status === "connecting" && (
                      <Loader2 className="w-3 h-3 text-yellow-400 animate-spin" />
                    )}
                    {forward.status === "error" && <AlertCircle className="w-3 h-3 text-red-400" />}
                    <span
                      className={`text-xs font-semibold ${
                        forward.status === "active"
                          ? "text-green-400"
                          : forward.status === "connecting"
                            ? "text-yellow-400"
                            : "text-red-400"
                      }`}
                    >
                      {forward.status === "active"
                        ? "LIVE"
                        : forward.status === "connecting"
                          ? "CONNECTING"
                          : "ERROR"}
                    </span>
                  </div>
                  <button
                    onClick={() => handleStopPortForward(forward.id)}
                    className="p-1 hover:bg-red-500/20 rounded transition text-red-400 hover:text-red-300"
                    title="Stop port forward"
                    aria-label="Stop port forward"
                  >
                    <X className="w-4 h-4" />
                  </button>
                </div>

                {/* Port Mapping */}
                <div className="space-y-1.5">
                  <div className="flex items-center gap-2 text-xs">
                    <span className="text-slate-400">Local:</span>
                    {forward.status === "active" ? (
                      <button
                        onClick={() => handleOpenInBrowser(forward.localAddress)}
                        className="flex items-center gap-1 text-accent hover:text-accent/80 hover:underline transition font-mono"
                      >
                        <span>127.0.0.1:{forward.localPort}</span>
                        <ExternalLink className="w-3 h-3" />
                      </button>
                    ) : (
                      <span className="text-slate-300 font-mono">
                        127.0.0.1:{forward.localPort}
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2 text-xs">
                    <span className="text-slate-400">Pod:</span>
                    <span className="text-slate-300 font-mono">
                      {podName}:{forward.podPort}
                    </span>
                  </div>
                  {forward.statusMessage && (
                    <div className="text-xs text-slate-500 mt-1">{forward.statusMessage}</div>
                  )}
                </div>
              </motion.div>
            ))}
          </AnimatePresence>
        )}
      </div>
    </motion.div>
  );
}
