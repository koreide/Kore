import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Bug, X } from "lucide-react";
import { addDebugContainer } from "@/lib/api";
import { formatError } from "@/lib/errors";
import { cn } from "@/lib/utils";

const DEBUG_IMAGES = [
  { label: "netshoot", value: "nicolaka/netshoot", description: "Network troubleshooting" },
  { label: "busybox", value: "busybox:latest", description: "Minimal Unix utilities" },
  { label: "ubuntu", value: "ubuntu:latest", description: "Full Ubuntu environment" },
  { label: "curl", value: "curlimages/curl", description: "cURL for HTTP debugging" },
] as const;

interface DebugContainerModalProps {
  open: boolean;
  onClose: () => void;
  onDebugReady: (containerName: string, image: string) => void;
  namespace: string;
  podName: string;
  containers: string[];
}

export function DebugContainerModal({
  open,
  onClose,
  onDebugReady,
  namespace,
  podName,
  containers,
}: DebugContainerModalProps) {
  const [image, setImage] = useState<string>(DEBUG_IMAGES[0].value);
  const [customImage, setCustomImage] = useState("");
  const [targetContainer, setTargetContainer] = useState("");
  const [shareProcessNamespace, setShareProcessNamespace] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const resolvedImage = image === "custom" ? customImage.trim() : image;

  const handleSubmit = async () => {
    if (!resolvedImage) {
      setError("Please specify a container image");
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const target = shareProcessNamespace && targetContainer ? targetContainer : undefined;
      const containerName = await addDebugContainer(namespace, podName, resolvedImage, target, [
        "/bin/sh",
      ]);
      onDebugReady(containerName, resolvedImage);
    } catch (err) {
      setError(formatError(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50"
            onClick={loading ? undefined : onClose}
          />

          <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
            <motion.div
              role="dialog"
              aria-modal="true"
              aria-label="Debug Container"
              initial={{ opacity: 0, scale: 0.95, y: 20 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.95, y: 20 }}
              className="bg-surface border border-accent/30 bg-accent/5 rounded-lg shadow-xl max-w-md w-full"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="p-6">
                {loading ? (
                  <div className="flex flex-col items-center justify-center py-10">
                    <div className="p-3 rounded-full bg-accent/20 mb-4">
                      <Bug className="w-8 h-8 text-accent animate-pulse" />
                    </div>
                    <h3 className="text-lg font-semibold text-slate-100 mb-2">
                      Creating debug container...
                    </h3>
                    <p className="text-slate-400 text-sm text-center mb-3">
                      Waiting for container to start. This may take up to 30 seconds.
                    </p>
                    <span className="font-mono text-xs text-accent/80 bg-accent/10 px-3 py-1 rounded">
                      {resolvedImage}
                    </span>
                  </div>
                ) : (
                  <>
                    <div className="flex items-start gap-4 mb-5">
                      <div className="p-2 rounded-full bg-accent/20">
                        <Bug className="w-6 h-6 text-accent" />
                      </div>
                      <div className="flex-1">
                        <h3 className="text-lg font-semibold text-slate-100">Debug Container</h3>
                        <p className="text-slate-400 text-xs mt-1">
                          Inject an ephemeral container into{" "}
                          <span className="font-mono text-accent">{podName}</span>
                        </p>
                      </div>
                      <button
                        onClick={onClose}
                        className="text-slate-400 hover:text-slate-200 transition"
                      >
                        <X className="w-5 h-5" />
                      </button>
                    </div>

                    {/* Image selector */}
                    <div className="mb-4">
                      <label className="block text-xs text-slate-400 mb-2 uppercase tracking-wider">
                        Debug Image
                      </label>
                      <div className="space-y-1.5">
                        {DEBUG_IMAGES.map((img) => (
                          <label
                            key={img.value}
                            className={cn(
                              "flex items-center gap-3 p-2.5 rounded-md border cursor-pointer transition",
                              image === img.value
                                ? "border-accent/50 bg-accent/10"
                                : "border-slate-800 hover:border-slate-700 hover:bg-muted/20",
                            )}
                          >
                            <input
                              type="radio"
                              name="debug-image"
                              value={img.value}
                              checked={image === img.value}
                              onChange={() => setImage(img.value)}
                              className="w-3.5 h-3.5 accent-[#58d0ff]"
                            />
                            <div className="flex-1 min-w-0">
                              <span className="text-sm text-slate-200">{img.label}</span>
                              <span className="text-xs text-slate-500 ml-2">{img.description}</span>
                            </div>
                            <span className="text-[10px] font-mono text-slate-600 truncate max-w-[140px]">
                              {img.value}
                            </span>
                          </label>
                        ))}
                        <label
                          className={cn(
                            "flex items-center gap-3 p-2.5 rounded-md border cursor-pointer transition",
                            image === "custom"
                              ? "border-accent/50 bg-accent/10"
                              : "border-slate-800 hover:border-slate-700 hover:bg-muted/20",
                          )}
                        >
                          <input
                            type="radio"
                            name="debug-image"
                            value="custom"
                            checked={image === "custom"}
                            onChange={() => setImage("custom")}
                            className="w-3.5 h-3.5 accent-[#58d0ff]"
                          />
                          <span className="text-sm text-slate-200">Custom</span>
                        </label>
                        {image === "custom" && (
                          <input
                            value={customImage}
                            onChange={(e) => setCustomImage(e.target.value)}
                            placeholder="e.g. alpine:latest"
                            className="w-full mt-1 px-3 py-2 rounded-md border border-slate-800 bg-background text-sm text-slate-200 placeholder:text-slate-600 outline-none focus:border-accent/50"
                          />
                        )}
                      </div>
                    </div>

                    {/* Share process namespace */}
                    <div className="mb-4">
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input
                          type="checkbox"
                          checked={shareProcessNamespace}
                          onChange={(e) => setShareProcessNamespace(e.target.checked)}
                          className="w-3.5 h-3.5 accent-[#58d0ff]"
                        />
                        <span className="text-sm text-slate-300">Share process namespace</span>
                      </label>
                      <p className="text-[10px] text-slate-500 mt-1 ml-5">
                        Target a container to share its PID namespace (see processes with ps)
                      </p>
                    </div>

                    {/* Target container dropdown */}
                    {shareProcessNamespace && containers.length > 0 && (
                      <div className="mb-4">
                        <label className="block text-xs text-slate-400 mb-1.5 uppercase tracking-wider">
                          Target Container
                        </label>
                        <select
                          value={targetContainer}
                          onChange={(e) => setTargetContainer(e.target.value)}
                          className="w-full px-3 py-2 rounded-md border border-slate-800 bg-background text-sm text-slate-200 outline-none focus:border-accent/50"
                        >
                          <option value="">Select a container...</option>
                          {containers.map((c) => (
                            <option key={c} value={c}>
                              {c}
                            </option>
                          ))}
                        </select>
                      </div>
                    )}

                    {/* Error display */}
                    {error && (
                      <div className="mb-4 p-2.5 bg-red-500/10 border border-red-500/50 rounded-md text-xs text-red-400">
                        {error}
                      </div>
                    )}

                    {/* Footer */}
                    <div className="flex gap-3 justify-end mt-6">
                      <button
                        onClick={onClose}
                        className="px-4 py-2 rounded border border-slate-800 hover:border-slate-700 hover:bg-muted/40 transition text-sm"
                      >
                        Cancel
                      </button>
                      <button
                        onClick={handleSubmit}
                        disabled={!resolvedImage}
                        className="flex items-center gap-2 px-4 py-2 rounded border border-accent/50 bg-accent/20 hover:bg-accent/30 transition text-sm text-accent disabled:opacity-50 disabled:cursor-not-allowed"
                      >
                        <Bug className="w-4 h-4" />
                        Create & Connect
                      </button>
                    </div>
                  </>
                )}
              </div>
            </motion.div>
          </div>
        </>
      )}
    </AnimatePresence>
  );
}
