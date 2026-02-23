import { useCallback, useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Bot, Send, X, Copy, Sparkles, Loader2 } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { cn } from "@/lib/utils";
import { useToast } from "./toast";

// ── Types ────────────────────────────────────────────────────────────────

export interface AIConfig {
  provider: "openai" | "anthropic" | "ollama";
  api_key?: string;
  model: string;
  base_url?: string;
}

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

interface AIResponsePayload {
  type: "chunk" | "done" | "error";
  content?: string;
  message?: string;
}

export interface DiagnoseRequest {
  context?: { kind: string; namespace: string; name: string };
  prompt: string;
}

interface AIPanelProps {
  open: boolean;
  onClose: () => void;
  resourceContext?: { kind: string; namespace: string; name: string };
}

// Placeholder for the backend invoke — imported from api.ts in production
async function aiDiagnose(_config: AIConfig, _request: DiagnoseRequest): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("ai_diagnose", { config: _config, request: _request });
}

// ── Helpers ──────────────────────────────────────────────────────────────

function loadAIConfig(): AIConfig {
  try {
    const stored = localStorage.getItem("kore-ai-config");
    if (stored) return JSON.parse(stored) as AIConfig;
  } catch {
    // ignore parse errors
  }
  return { provider: "openai", model: "gpt-4o" };
}

let sessionCounter = 0;
function nextSessionId(): string {
  sessionCounter += 1;
  return `ai-session-${Date.now()}-${sessionCounter}`;
}

// ── Quick Actions ────────────────────────────────────────────────────────

const quickActions = [
  { label: "Diagnose", prompt: "Diagnose this resource and identify any issues." },
  {
    label: "Why is this failing?",
    prompt: "Why is this resource failing? Analyze the status, events, and conditions.",
  },
  { label: "Suggest fixes", prompt: "Suggest fixes for the current issues with this resource." },
];

// ── Component ────────────────────────────────────────────────────────────

export function AIPanel({ open, onClose, resourceContext }: AIPanelProps) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const [sessionId, setSessionId] = useState(() => nextSessionId());

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const toast = useToast();

  // Auto-scroll to bottom when messages change
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  // Focus input when panel opens
  useEffect(() => {
    if (open) {
      setTimeout(() => inputRef.current?.focus(), 200);
    }
  }, [open]);

  // Esc key to close
  useEffect(() => {
    if (!open) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [open, onClose]);

  // Listen for AI streaming response events
  useEffect(() => {
    if (!open) return;

    let unlistenFn: (() => void) | null = null;
    let isMounted = true;

    const setupListener = async () => {
      const eventName = `ai-response://${sessionId}`;
      const unlisten = await listen<AIResponsePayload>(eventName, (event) => {
        if (!isMounted) return;
        const payload = event.payload;

        if (payload.type === "chunk" && payload.content) {
          setMessages((prev) => {
            const last = prev[prev.length - 1];
            if (last && last.role === "assistant") {
              const updated = [...prev];
              updated[updated.length - 1] = {
                ...last,
                content: last.content + payload.content,
              };
              return updated;
            }
            return [...prev, { role: "assistant", content: payload.content! }];
          });
        } else if (payload.type === "done") {
          setIsStreaming(false);
        } else if (payload.type === "error") {
          setIsStreaming(false);
          const errorMsg = payload.message || "An unknown error occurred.";
          setMessages((prev) => {
            const last = prev[prev.length - 1];
            if (last && last.role === "assistant") {
              const updated = [...prev];
              updated[updated.length - 1] = {
                ...last,
                content: last.content + `\n\n[Error: ${errorMsg}]`,
              };
              return updated;
            }
            return [...prev, { role: "assistant", content: `[Error: ${errorMsg}]` }];
          });
        }
      });

      if (!isMounted) {
        unlisten();
        return;
      }
      unlistenFn = unlisten;
    };

    setupListener().catch((err) => {
      console.error("Failed to set up AI response listener", err);
    });

    return () => {
      isMounted = false;
      if (unlistenFn) unlistenFn();
    };
  }, [open, sessionId]);

  const sendMessage = useCallback(
    async (text: string) => {
      if (!text.trim() || isStreaming) return;

      const userMessage: ChatMessage = { role: "user", content: text.trim() };
      setMessages((prev) => [...prev, userMessage]);
      setInput("");
      setIsStreaming(true);

      // Create a new session id for this request
      const newSessionId = nextSessionId();
      setSessionId(newSessionId);

      const config = loadAIConfig();

      try {
        await aiDiagnose(config, {
          context: resourceContext,
          prompt: text.trim(),
        });
      } catch (err) {
        setIsStreaming(false);
        const errMsg = err instanceof Error ? err.message : String(err);
        setMessages((prev) => [...prev, { role: "assistant", content: `[Error: ${errMsg}]` }]);
      }
    },
    [isStreaming, resourceContext],
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    sendMessage(input);
  };

  const handleQuickAction = (prompt: string) => {
    sendMessage(prompt);
  };

  const handleCopyMessage = async (content: string) => {
    try {
      await navigator.clipboard.writeText(content);
      toast("Copied to clipboard", "success");
    } catch {
      toast("Failed to copy", "error");
    }
  };

  const handleClearChat = () => {
    setMessages([]);
    setSessionId(nextSessionId());
  };

  return (
    <AnimatePresence>
      {open && (
        <>
          {/* Backdrop */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="fixed inset-0 z-40 bg-black/30"
            onClick={onClose}
          />

          {/* Panel */}
          <motion.div
            initial={{ x: "100%" }}
            animate={{ x: 0 }}
            exit={{ x: "100%" }}
            transition={{ type: "spring", stiffness: 400, damping: 35 }}
            className="fixed top-0 right-0 bottom-0 z-50 w-[420px] max-w-[90vw] flex flex-col bg-surface border-l border-slate-800 shadow-2xl"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-slate-800 bg-surface/80">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center">
                  <Bot className="w-4.5 h-4.5 text-accent" />
                </div>
                <div>
                  <h2 className="text-sm font-semibold text-slate-100">AI Assistant</h2>
                  <p className="text-[10px] text-slate-500">Kubernetes troubleshooting</p>
                </div>
              </div>
              <div className="flex items-center gap-1">
                {messages.length > 0 && (
                  <button
                    onClick={handleClearChat}
                    className="px-2 py-1 rounded text-[10px] text-slate-500 hover:text-slate-300 hover:bg-muted/50 transition"
                  >
                    Clear
                  </button>
                )}
                <button
                  onClick={onClose}
                  className="p-1.5 rounded-md hover:bg-muted/50 transition text-slate-400 hover:text-slate-200"
                  aria-label="Close AI panel"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>
            </div>

            {/* Resource Context Indicator */}
            {resourceContext && (
              <div className="px-4 py-2 border-b border-slate-800/50 bg-accent/5">
                <div className="flex items-center gap-2 text-xs">
                  <Sparkles className="w-3.5 h-3.5 text-accent shrink-0" />
                  <span className="text-slate-400">Context:</span>
                  <span className="font-mono text-accent truncate">
                    {resourceContext.kind}/{resourceContext.namespace}/{resourceContext.name}
                  </span>
                </div>
              </div>
            )}

            {/* Quick Actions */}
            {messages.length === 0 && (
              <div className="px-4 py-3 border-b border-slate-800/50">
                <p className="text-[10px] uppercase tracking-wider text-slate-500 mb-2">
                  Quick Actions
                </p>
                <div className="flex flex-wrap gap-1.5">
                  {quickActions.map((action) => (
                    <button
                      key={action.label}
                      onClick={() => handleQuickAction(action.prompt)}
                      disabled={isStreaming}
                      className={cn(
                        "px-3 py-1.5 rounded-lg border text-xs transition",
                        "border-slate-700 text-slate-300 hover:border-accent/50 hover:text-accent hover:bg-accent/5",
                        "disabled:opacity-40 disabled:cursor-not-allowed",
                      )}
                    >
                      {action.label}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {/* Messages */}
            <div className="flex-1 overflow-y-auto px-4 py-3 space-y-4">
              {messages.length === 0 && (
                <div className="flex flex-col items-center justify-center h-full text-center px-6">
                  <div className="w-14 h-14 rounded-2xl bg-accent/10 flex items-center justify-center mb-4">
                    <Bot className="w-7 h-7 text-accent/60" />
                  </div>
                  <p className="text-sm text-slate-400 mb-1">Ask about your Kubernetes resources</p>
                  <p className="text-xs text-slate-600">
                    Diagnose issues, understand failures, and get fix suggestions.
                  </p>
                </div>
              )}

              {messages.map((msg, idx) => (
                <div
                  key={idx}
                  className={cn(
                    "flex gap-2.5",
                    msg.role === "user" ? "justify-end" : "justify-start",
                  )}
                >
                  {msg.role === "assistant" && (
                    <div className="w-6 h-6 rounded-md bg-accent/15 flex items-center justify-center shrink-0 mt-0.5">
                      <Bot className="w-3.5 h-3.5 text-accent" />
                    </div>
                  )}

                  <div
                    className={cn(
                      "relative group max-w-[85%] rounded-lg px-3 py-2 text-sm leading-relaxed",
                      msg.role === "user"
                        ? "bg-accent/15 text-slate-100 rounded-br-sm"
                        : "bg-muted/60 text-slate-200 border border-slate-800/50 rounded-bl-sm",
                    )}
                  >
                    <pre className="whitespace-pre-wrap font-sans text-sm break-words">
                      {msg.content}
                    </pre>

                    {/* Copy button on assistant messages */}
                    {msg.role === "assistant" && (
                      <button
                        onClick={() => handleCopyMessage(msg.content)}
                        className="absolute -top-2 -right-2 p-1 rounded bg-surface border border-slate-700 opacity-0 group-hover:opacity-100 transition-opacity hover:border-accent/50"
                        aria-label="Copy message"
                      >
                        <Copy className="w-3 h-3 text-slate-400" />
                      </button>
                    )}
                  </div>
                </div>
              ))}

              {/* Streaming indicator */}
              {isStreaming && (
                <div className="flex items-center gap-2 text-xs text-slate-500">
                  <Loader2 className="w-3.5 h-3.5 animate-spin text-accent" />
                  <span>Thinking...</span>
                </div>
              )}

              <div ref={messagesEndRef} />
            </div>

            {/* Input */}
            <form
              onSubmit={handleSubmit}
              className="px-4 py-3 border-t border-slate-800 bg-surface/80"
            >
              <div className="flex items-center gap-2">
                <input
                  ref={inputRef}
                  type="text"
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  placeholder={
                    isStreaming ? "Waiting for response..." : "Ask about this resource..."
                  }
                  disabled={isStreaming}
                  className={cn(
                    "flex-1 px-3 py-2 rounded-lg bg-background border border-slate-800 text-sm text-slate-100 placeholder-slate-600",
                    "focus:outline-none focus:border-accent/50 transition",
                    "disabled:opacity-50 disabled:cursor-not-allowed",
                  )}
                />
                <button
                  type="submit"
                  disabled={isStreaming || !input.trim()}
                  className={cn(
                    "p-2 rounded-lg transition",
                    input.trim() && !isStreaming
                      ? "bg-accent/20 text-accent hover:bg-accent/30 border border-accent/50"
                      : "bg-muted/30 text-slate-600 border border-slate-800 cursor-not-allowed",
                  )}
                  aria-label="Send message"
                >
                  <Send className="w-4 h-4" />
                </button>
              </div>
              <p className="mt-1.5 text-[10px] text-slate-600 text-center">
                AI responses may be inaccurate. Always verify before acting.
              </p>
            </form>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
