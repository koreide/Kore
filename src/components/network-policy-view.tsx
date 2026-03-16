import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  Loader2,
  Inbox,
  Shield,
  ShieldX,
  RefreshCw,
  Search,
  X,
  ChevronDown,
  ChevronRight,
  Send,
  Bot,
  Trash2,
  ArrowRight,
  CheckCircle2,
  XCircle,
  ChevronUp,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { cn } from "@/lib/utils";
import { useNetworkPolicyGraph } from "@/hooks/use-network-policy-graph";
import { useToast } from "./toast";
import { useAIConfig } from "@/hooks/use-ai-config";
import { AIConfigWarning } from "./ai-config-warning";
import { ChatMessageBubble } from "./chat-message-bubble";
import type {
  NetworkPolicySummary,
  TrafficSimulationResult,
  NetworkPolicyGraph,
  NetworkPolicyPodGroup,
  AIChatRequest,
  ChatMessage,
} from "@/lib/api";
import { aiChat } from "@/lib/api";

// ── Types ────────────────────────────────────────────────────────────

interface NPMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  simulationResult?: TrafficSimulationResult;
  sourceLabel?: string;
  destLabel?: string;
  relatedPolicies?: NetworkPolicySummary[];
}

interface AIResponsePayload {
  type: "chunk" | "done" | "error" | "status";
  content?: string;
  message?: string;
}

// ── Helpers ──────────────────────────────────────────────────────────

let sessionCounter = 0;
function nextSessionId(): string {
  sessionCounter += 1;
  return `np-ai-${Date.now()}-${sessionCounter}`;
}

function msgId(): string {
  return `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

// ── Build graph context for AI system prompt ─────────────────────────

function buildGraphContext(graph: NetworkPolicyGraph): string {
  const lines: string[] = [];

  lines.push("# Network Policy Graph Context\n");

  // Policies
  if (graph.policies.length > 0) {
    lines.push(`## Policies (${graph.policies.length})`);
    for (const p of graph.policies) {
      const types = p.policy_types.join(", ");
      const selector =
        Object.keys(p.pod_selector).length > 0
          ? Object.entries(p.pod_selector)
              .map(([k, v]) => `${k}=${v}`)
              .join(", ")
          : "all pods";
      lines.push(
        `- **${p.name}** (ns: ${p.namespace}) — types: [${types}], selector: {${selector}}, affects ${p.affected_pod_count} pods, ${p.ingress_rule_count} ingress rules, ${p.egress_rule_count} egress rules`,
      );
    }
    lines.push("");
  }

  // Workload groups
  if (graph.groups.length > 0) {
    lines.push(`## Workload Groups (${graph.groups.length})`);
    for (const g of graph.groups) {
      const isolation: string[] = [];
      if (g.is_isolated_ingress) isolation.push("ingress-isolated");
      if (g.is_isolated_egress) isolation.push("egress-isolated");
      const isoStr = isolation.length > 0 ? ` [${isolation.join(", ")}]` : "";
      const policies =
        g.matching_policies.length > 0 ? ` policies: [${g.matching_policies.join(", ")}]` : "";
      lines.push(
        `- **${g.name}** (ns: ${g.namespace}, kind: ${g.kind}, ${g.pod_count} pods)${isoStr}${policies}`,
      );
    }
    lines.push("");
  }

  // CIDR nodes
  if (graph.external_cidrs.length > 0) {
    lines.push(`## External CIDRs (${graph.external_cidrs.length})`);
    for (const c of graph.external_cidrs) {
      const except = c.except.length > 0 ? ` except [${c.except.join(", ")}]` : "";
      lines.push(`- ${c.cidr}${except} (from policy: ${c.from_policy})`);
    }
    lines.push("");
  }

  // Traffic edges
  if (graph.edges.length > 0) {
    lines.push(`## Traffic Edges (${graph.edges.length})`);
    for (const e of graph.edges) {
      const ports =
        e.ports.length > 0
          ? e.ports.map((p) => (p.port ? `${p.port}/${p.protocol}` : p.protocol)).join(", ")
          : "all ports";
      lines.push(`- ${e.source} → ${e.target} [${e.direction}] via ${e.policy_name} (${ports})`);
    }
    lines.push("");
  }

  return lines.join("\n");
}

// ── Detect simulation from user question ─────────────────────────────

interface SimulationMatch {
  sourceGroup: NetworkPolicyPodGroup;
  destGroup: NetworkPolicyPodGroup;
}

function detectSimulation(text: string, groups: NetworkPolicyPodGroup[]): SimulationMatch | null {
  // Pattern: "can X talk/reach/connect/access Y"
  const pattern =
    /can\s+(.+?)\s+(?:talk|reach|connect|access|communicate|send)\s+(?:to|with)\s+(.+?)[\s?]*$/i;
  const match = text.match(pattern);
  if (!match) return null;

  const sourceQuery = match[1].trim().toLowerCase();
  const destQuery = match[2].trim().toLowerCase();

  const findGroup = (query: string): NetworkPolicyPodGroup | undefined => {
    // Exact name match
    let found = groups.find((g) => g.name.toLowerCase() === query);
    if (found) return found;
    // Partial match
    found = groups.find((g) => g.name.toLowerCase().includes(query));
    if (found) return found;
    // Match by pod name
    found = groups.find((g) => g.pods.some((p) => p.name.toLowerCase().includes(query)));
    return found;
  };

  const sourceGroup = findGroup(sourceQuery);
  const destGroup = findGroup(destQuery);

  if (sourceGroup && destGroup && sourceGroup.id !== destGroup.id) {
    return { sourceGroup, destGroup };
  }
  return null;
}

// ── Main Component ───────────────────────────────────────────────────

interface NetworkPolicyViewProps {
  namespace?: string;
  currentContext?: string;
  onGoToSettings?: () => void;
}

export function NetworkPolicyView({ namespace, currentContext, onGoToSettings }: NetworkPolicyViewProps) {
  const { graph, loading, error, refresh, simulate } = useNetworkPolicyGraph(
    namespace,
    currentContext,
  );

  // Chat state
  const [messages, setMessages] = useState<NPMessage[]>([]);
  const [input, setInput] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const { aiConfig, isConfigured: hasAIConfig } = useAIConfig();

  // Policy sidebar
  const [selectedPolicy, setSelectedPolicy] = useState<string | null>(null);
  const [policySearch, setPolicySearch] = useState("");
  const [policyPanelOpen, setPolicyPanelOpen] = useState(true);

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const toast = useToast();

  // Auto-scroll only when user is already near the bottom
  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) return;
    const { scrollTop, scrollHeight, clientHeight } = container;
    const distanceFromBottom = scrollHeight - scrollTop - clientHeight;
    // Only auto-scroll if within 150px of bottom
    if (distanceFromBottom < 150) {
      messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [messages]);

  // Focus input on mount
  useEffect(() => {
    const timer = setTimeout(() => inputRef.current?.focus(), 300);
    return () => clearTimeout(timer);
  }, []);

  // Cleanup listener on unmount
  useEffect(() => {
    return () => {
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    };
  }, []);

  // ── Data memos ────────────────────────────────────────────────────

  const filteredPolicies = useMemo(() => {
    if (!graph) return [];
    if (!policySearch) return graph.policies;
    const q = policySearch.toLowerCase();
    return graph.policies.filter(
      (p) => p.name.toLowerCase().includes(q) || p.namespace.toLowerCase().includes(q),
    );
  }, [graph, policySearch]);

  const allPods = useMemo(() => {
    if (!graph) return [];
    const pods: { name: string; namespace: string }[] = [];
    for (const g of graph.groups) {
      for (const p of g.pods) {
        pods.push({ name: p.name, namespace: p.namespace });
      }
    }
    return pods;
  }, [graph]);

  // ── Suggested questions ───────────────────────────────────────────

  const suggestedQuestions = useMemo(() => {
    if (!graph) return [];
    const questions: string[] = [];

    // If 2+ groups: connectivity question
    if (graph.groups.length >= 2) {
      const g1 = graph.groups[0];
      const g2 = graph.groups[1];
      questions.push(`Can ${g1.name} talk to ${g2.name}?`);
    }

    // Isolated pods
    const isolated = graph.groups.filter((g) => g.is_isolated_ingress || g.is_isolated_egress);
    if (isolated.length > 0) {
      questions.push("Which pods are isolated and why?");
    }

    // CIDR edges
    if (graph.external_cidrs.length > 0) {
      questions.push("What external traffic is allowed?");
    }

    // Policy explanation
    if (graph.policies.length > 0) {
      questions.push(`What does ${graph.policies[0].name} do?`);
    }

    // Always
    questions.push("Summarize the network security posture");

    return questions.slice(0, 4);
  }, [graph]);

  // ── Send message ──────────────────────────────────────────────────

  const sendMessage = useCallback(
    async (text: string) => {
      if (!text.trim() || isStreaming || !graph) return;

      const trimmed = text.trim();
      const userMsg: NPMessage = { id: msgId(), role: "user", content: trimmed };

      // Detect simulation
      const simMatch = detectSimulation(trimmed, graph.groups);
      if (simMatch) {
        const sourcePod = simMatch.sourceGroup.pods[0];
        const destPod = simMatch.destGroup.pods[0];

        if (sourcePod && destPod) {
          try {
            const result = await simulate(
              sourcePod.namespace,
              sourcePod.name,
              destPod.namespace,
              destPod.name,
            );
            userMsg.simulationResult = result;
            userMsg.sourceLabel = simMatch.sourceGroup.name;
            userMsg.destLabel = simMatch.destGroup.name;
          } catch {
            // Simulation failed, continue without it
          }
        }
      }

      setMessages((prev) => [...prev, userMsg]);
      setInput("");
      setIsStreaming(true);

      // Build messages for AI
      const systemContext = buildGraphContext(graph);
      const systemPrompt = `You are a Kubernetes network policy expert assistant. You analyze NetworkPolicies and help users understand traffic flow between workloads.

Answer questions clearly and concisely. Reference specific policy names when relevant. Use the graph context below to answer accurately.

${systemContext}

When a simulation result is provided with the user's question, explain WHY the traffic is allowed or denied based on the policies. Reference the specific policies and rules.

Available workloads: ${graph.groups.map((g) => g.name).join(", ")}
Total pods: ${allPods.length}`;

      const chatMessages: ChatMessage[] = [{ role: "system", content: systemPrompt }];

      // Add conversation history (last 10 messages)
      const recentMsgs = [...messages, userMsg].slice(-10);
      for (const m of recentMsgs) {
        let content = m.content;
        if (m.simulationResult) {
          content += `\n\n[Simulation Result: ${m.sourceLabel} → ${m.destLabel}: ${m.simulationResult.allowed ? "ALLOWED" : "DENIED"}. ${m.simulationResult.summary}]`;
        }
        chatMessages.push({ role: m.role, content });
      }

      const newSessionId = nextSessionId();

      // Clean up previous listener
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }

      // Set up listener BEFORE calling backend
      try {
        const eventName = `ai-chat://${newSessionId}`;
        const unlisten = await listen<AIResponsePayload>(eventName, (event) => {
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
              return [...prev, { id: msgId(), role: "assistant", content: payload.content! }];
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
              return [...prev, { id: msgId(), role: "assistant", content: `[Error: ${errorMsg}]` }];
            });
          }
          // status events are ignored (could show "Thinking..." but keeping it simple)
        });
        unlistenRef.current = unlisten;
      } catch (err) {
        console.error("Failed to set up AI response listener", err);
        setIsStreaming(false);
        return;
      }

      // Call backend
      try {
        const request: AIChatRequest = {
          messages: chatMessages,
          session_id: newSessionId,
          namespace: namespace,
        };
        await aiChat(aiConfig, request);
      } catch (err) {
        setIsStreaming(false);
        const errMsg = err instanceof Error ? err.message : String(err);
        setMessages((prev) => [
          ...prev,
          { id: msgId(), role: "assistant", content: `[Error: ${errMsg}]` },
        ]);
      }
    },
    [isStreaming, graph, messages, allPods, namespace, aiConfig, simulate],
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    sendMessage(input);
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
  };

  // ── Render states ─────────────────────────────────────────────────

  if (loading && !graph) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center gap-3">
        <Loader2 className="w-8 h-8 text-accent animate-spin" />
        <p className="text-sm text-slate-400">Loading network policies...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center gap-3 text-red-400">
        <ShieldX className="w-10 h-10" />
        <p className="text-sm font-medium">Failed to load network policies</p>
        <p className="text-xs text-slate-500 max-w-md text-center">{error}</p>
      </div>
    );
  }

  if (!graph || (graph.groups.length === 0 && graph.policies.length === 0)) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center text-slate-400 gap-3">
        <Inbox className="w-12 h-12 text-slate-600" />
        <p className="text-sm font-medium">No network policies found</p>
        <p className="text-xs text-slate-500">
          {namespace
            ? `No NetworkPolicies in "${namespace}"`
            : "Select a namespace to view network policies"}
        </p>
      </div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="h-full w-full relative overflow-hidden flex flex-col"
    >
      {/* Stats Bar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-slate-800/50 shrink-0">
        <div className="flex items-center gap-4">
          <div className="text-[10px] text-slate-500 font-mono flex items-center gap-1.5">
            <Shield className="w-3 h-3" />
            {graph.policies.length} policies
          </div>
          <div className="text-[10px] text-slate-500 font-mono">
            {graph.groups.length} workloads
          </div>
          <div className="text-[10px] text-slate-500 font-mono">{graph.edges.length} edges</div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={refresh}
            className="flex items-center gap-1.5 px-2 py-1 text-[11px] text-slate-400 bg-surface/80 border border-slate-800 rounded-md hover:border-accent/50 hover:text-slate-300 transition"
            title="Refresh"
          >
            <RefreshCw className="w-3 h-3" />
          </button>
        </div>
      </div>

      {/* Main content */}
      <div className="flex-1 flex min-h-0 overflow-hidden">
        {/* Chat area */}
        <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
          {/* Messages */}
          <div ref={scrollContainerRef} className="flex-1 overflow-y-auto px-4 py-3 space-y-4">
            {messages.length === 0 && (
              <div className="flex flex-col items-center justify-center h-full text-center px-6">
                <div className="w-14 h-14 rounded-2xl bg-accent/10 flex items-center justify-center mb-4">
                  <Bot className="w-7 h-7 text-accent/60" />
                </div>
                <p className="text-sm text-slate-400 mb-1">Ask about your network policies</p>
                <p className="text-xs text-slate-600 mb-6">
                  "Can X talk to Y?", "Which pods are isolated?", "What does this policy do?"
                </p>

                {!hasAIConfig && (
                  <AIConfigWarning className="mb-6 rounded-lg border border-amber-500/30 max-w-sm" onGoToSettings={onGoToSettings} />
                )}

                {/* Suggested questions */}
                {suggestedQuestions.length > 0 && (
                  <div className="flex flex-wrap gap-2 justify-center max-w-lg">
                    {suggestedQuestions.map((q) => (
                      <button
                        key={q}
                        onClick={() => sendMessage(q)}
                        disabled={isStreaming || !hasAIConfig}
                        className={cn(
                          "px-3 py-1.5 rounded-lg border text-xs transition text-left",
                          "border-slate-700 text-slate-300 hover:border-accent/50 hover:text-accent hover:bg-accent/5",
                          "disabled:opacity-40 disabled:cursor-not-allowed",
                        )}
                      >
                        {q}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}

            {messages.map((msg) => (
              <div key={msg.id}>
                <ChatMessageBubble
                  role={msg.role}
                  content={msg.content}
                  onCopy={msg.role === "assistant" ? handleCopyMessage : undefined}
                />

                {/* Simulation result card (attached to user message) */}
                {msg.role === "user" && msg.simulationResult && (
                  <div className="mt-2 flex justify-end">
                    <SimulationResultCard
                      result={msg.simulationResult}
                      sourceLabel={msg.sourceLabel || "Source"}
                      destLabel={msg.destLabel || "Destination"}
                    />
                  </div>
                )}
              </div>
            ))}

            {/* Streaming indicator */}
            {isStreaming && messages[messages.length - 1]?.role !== "assistant" && (
              <div className="flex items-center gap-2 text-xs text-slate-500">
                <Loader2 className="w-3.5 h-3.5 animate-spin text-accent" />
                <span>Thinking...</span>
              </div>
            )}

            <div ref={messagesEndRef} />
          </div>

          {/* Suggested questions (when conversation started) */}
          {messages.length > 0 && suggestedQuestions.length > 0 && (
            <div className="px-4 py-2 border-t border-slate-800/30 shrink-0">
              <div className="flex gap-1.5 overflow-x-auto">
                {suggestedQuestions.map((q) => (
                  <button
                    key={q}
                    onClick={() => sendMessage(q)}
                    disabled={isStreaming || !hasAIConfig}
                    className={cn(
                      "px-2.5 py-1 rounded-md border text-[10px] transition whitespace-nowrap shrink-0",
                      "border-slate-800 text-slate-400 hover:border-accent/50 hover:text-accent",
                      "disabled:opacity-40 disabled:cursor-not-allowed",
                    )}
                  >
                    {q}
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Input bar */}
          <form
            onSubmit={handleSubmit}
            className="px-4 py-3 border-t border-slate-800 bg-surface/80 shrink-0"
          >
            <div className="flex items-center gap-2">
              {messages.length > 0 && (
                <button
                  type="button"
                  onClick={handleClearChat}
                  disabled={isStreaming}
                  className={cn(
                    "p-2 rounded-lg transition shrink-0",
                    "text-slate-500 hover:text-red-400 hover:bg-muted/50",
                    "disabled:opacity-40 disabled:cursor-not-allowed",
                  )}
                  title="Clear chat"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              )}
              <input
                ref={inputRef}
                type="text"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                placeholder={
                  !hasAIConfig
                    ? "Configure an AI provider to get started..."
                    : isStreaming
                      ? "Waiting for response..."
                      : "Ask about network policies..."
                }
                disabled={isStreaming || !hasAIConfig}
                className={cn(
                  "flex-1 px-3 py-2 rounded-lg bg-background border border-slate-800 text-sm text-slate-100 placeholder-slate-600",
                  "focus:outline-none focus:border-accent/50 transition",
                  "disabled:opacity-50 disabled:cursor-not-allowed",
                )}
              />
              <button
                type="submit"
                disabled={isStreaming || !input.trim() || !hasAIConfig}
                className={cn(
                  "p-2 rounded-lg transition shrink-0",
                  input.trim() && !isStreaming && hasAIConfig
                    ? "bg-accent/20 text-accent hover:bg-accent/30 border border-accent/50"
                    : "bg-muted/30 text-slate-600 border border-slate-800 cursor-not-allowed",
                )}
                aria-label="Send message"
              >
                <Send className="w-4 h-4" />
              </button>
            </div>
            <p className="mt-1.5 text-[10px] text-slate-600 text-center">
              AI responses may be inaccurate. Simulation results are computed locally.
            </p>
          </form>
        </div>

        {/* Policy List Panel */}
        <div
          className={`relative transition-all duration-300 shrink-0 ${policyPanelOpen ? "w-[300px]" : "w-0"}`}
        >
          <button
            onClick={() => setPolicyPanelOpen(!policyPanelOpen)}
            className="absolute -left-6 top-3 z-30 w-6 h-8 bg-surface/90 border border-slate-800 border-r-0 rounded-l-md flex items-center justify-center text-slate-500 hover:text-slate-300 transition"
          >
            {policyPanelOpen ? (
              <ChevronDown className="w-3 h-3 rotate-[-90deg]" />
            ) : (
              <ChevronRight className="w-3 h-3 rotate-[-90deg]" />
            )}
          </button>

          {policyPanelOpen && (
            <div className="w-[300px] h-full border-l border-slate-800 bg-surface/80 glass flex flex-col overflow-hidden">
              <div className="p-3 border-b border-slate-800/50">
                <h3 className="text-xs font-medium text-slate-300 mb-2 flex items-center gap-1.5">
                  <Shield className="w-3.5 h-3.5 text-accent" />
                  Network Policies ({graph.policies.length})
                </h3>
                <div className="relative">
                  <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-slate-500" />
                  <input
                    value={policySearch}
                    onChange={(e) => setPolicySearch(e.target.value)}
                    placeholder="Filter policies..."
                    className="w-full pl-6 pr-6 py-1 text-[11px] font-mono bg-background border border-slate-800 rounded placeholder:text-slate-600 text-slate-300 focus:outline-none focus:border-accent/50 transition"
                  />
                  {policySearch && (
                    <button
                      onClick={() => setPolicySearch("")}
                      className="absolute right-1.5 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  )}
                </div>
              </div>
              <div className="flex-1 overflow-y-auto p-2 space-y-1.5">
                {filteredPolicies.map((policy) => (
                  <PolicyCard
                    key={`${policy.namespace}/${policy.name}`}
                    policy={policy}
                    isSelected={selectedPolicy === policy.name}
                    onClick={() =>
                      setSelectedPolicy(selectedPolicy === policy.name ? null : policy.name)
                    }
                  />
                ))}
                {filteredPolicies.length === 0 && (
                  <p className="text-[10px] text-slate-600 text-center py-4">
                    No matching policies
                  </p>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </motion.div>
  );
}

// ── Simulation Result Card ──────────────────────────────────────────

function SimulationResultCard({
  result,
  sourceLabel,
  destLabel,
}: {
  result: TrafficSimulationResult;
  sourceLabel: string;
  destLabel: string;
}) {
  const [expanded, setExpanded] = useState(false);

  const egressResults = result.egress_evaluation.policy_results.filter((r) => r.selects_pod);
  const ingressResults = result.ingress_evaluation.policy_results.filter((r) => r.selects_pod);
  const hasDetails = egressResults.length > 0 || ingressResults.length > 0;

  return (
    <div
      className={cn(
        "max-w-[85%] rounded-lg border text-xs overflow-hidden",
        result.allowed
          ? "border-emerald-500/30 bg-emerald-500/5"
          : "border-red-500/30 bg-red-500/5",
      )}
    >
      {/* Header: source → dest + badge */}
      <div className="flex items-center justify-between px-3 py-2 gap-3">
        <div className="flex items-center gap-2 font-mono text-slate-300 min-w-0">
          <span className="truncate">{sourceLabel}</span>
          <ArrowRight className="w-3.5 h-3.5 text-slate-500 shrink-0" />
          <span className="truncate">{destLabel}</span>
        </div>
        <span
          className={cn(
            "px-2 py-0.5 rounded font-semibold text-[10px] shrink-0 flex items-center gap-1",
            result.allowed ? "bg-emerald-500/20 text-emerald-400" : "bg-red-500/20 text-red-400",
          )}
        >
          {result.allowed ? <CheckCircle2 className="w-3 h-3" /> : <XCircle className="w-3 h-3" />}
          {result.allowed ? "ALLOWED" : "DENIED"}
        </span>
      </div>

      {/* Summary */}
      <div className="px-3 pb-2 text-slate-400">{result.summary}</div>

      {/* Egress/Ingress quick summary */}
      <div className="px-3 pb-2 flex gap-3">
        <div className="flex items-center gap-1">
          <span className="text-slate-500">Egress:</span>
          {result.egress_evaluation.isolated ? (
            <span className="text-amber-400">Isolated</span>
          ) : egressResults.some((r) => r.allows_traffic) ? (
            <span className="text-emerald-400 flex items-center gap-0.5">
              <CheckCircle2 className="w-2.5 h-2.5" /> Allowed
            </span>
          ) : egressResults.length > 0 ? (
            <span className="text-red-400 flex items-center gap-0.5">
              <XCircle className="w-2.5 h-2.5" /> Denied
            </span>
          ) : (
            <span className="text-slate-500">No policy</span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <span className="text-slate-500">Ingress:</span>
          {result.ingress_evaluation.isolated ? (
            <span className="text-amber-400">Isolated</span>
          ) : ingressResults.some((r) => r.allows_traffic) ? (
            <span className="text-emerald-400 flex items-center gap-0.5">
              <CheckCircle2 className="w-2.5 h-2.5" /> Allowed
            </span>
          ) : ingressResults.length > 0 ? (
            <span className="text-red-400 flex items-center gap-0.5">
              <XCircle className="w-2.5 h-2.5" /> Denied
            </span>
          ) : (
            <span className="text-slate-500">No policy</span>
          )}
        </div>
      </div>

      {/* Expandable policy details */}
      {hasDetails && (
        <>
          <button
            onClick={() => setExpanded(!expanded)}
            className="w-full px-3 py-1.5 flex items-center gap-1 text-[10px] text-slate-500 hover:text-slate-300 border-t border-slate-800/30 transition"
          >
            {expanded ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
            Policy details ({egressResults.length + ingressResults.length} evaluations)
          </button>

          <AnimatePresence>
            {expanded && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                className="overflow-hidden"
              >
                <div className="px-3 pb-2 space-y-1">
                  {egressResults.map((r) => (
                    <div
                      key={`e-${r.policy_name}`}
                      className={cn(
                        "px-2 py-1 rounded text-[10px]",
                        r.allows_traffic
                          ? "bg-emerald-500/10 text-emerald-400"
                          : "bg-red-500/10 text-red-400",
                      )}
                    >
                      <span className="text-slate-500">egress</span> {r.policy_name}: {r.reason}
                    </div>
                  ))}
                  {ingressResults.map((r) => (
                    <div
                      key={`i-${r.policy_name}`}
                      className={cn(
                        "px-2 py-1 rounded text-[10px]",
                        r.allows_traffic
                          ? "bg-emerald-500/10 text-emerald-400"
                          : "bg-red-500/10 text-red-400",
                      )}
                    >
                      <span className="text-slate-500">ingress</span> {r.policy_name}: {r.reason}
                    </div>
                  ))}
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </>
      )}
    </div>
  );
}

// ── Policy Card ──────────────────────────────────────────────────────

function PolicyCard({
  policy,
  isSelected,
  onClick,
}: {
  policy: NetworkPolicySummary;
  isSelected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`w-full text-left p-2.5 rounded-lg border transition ${
        isSelected
          ? "border-accent/50 bg-accent/10"
          : "border-slate-800/50 bg-background/50 hover:border-slate-700"
      }`}
    >
      <div className="flex items-center gap-1.5 mb-1.5">
        <Shield className="w-3 h-3 text-accent flex-shrink-0" />
        <span className="text-[11px] font-medium text-slate-200 truncate">{policy.name}</span>
      </div>
      <div className="text-[10px] text-slate-500 mb-1.5">ns: {policy.namespace}</div>
      <div className="flex flex-wrap gap-1 mb-1.5">
        {policy.policy_types.map((t) => (
          <span
            key={t}
            className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${
              t === "Ingress"
                ? "bg-emerald-500/15 text-emerald-400"
                : "bg-blue-500/15 text-blue-400"
            }`}
          >
            {t}
          </span>
        ))}
        <span className="px-1.5 py-0.5 rounded text-[9px] bg-slate-800 text-slate-400">
          {policy.affected_pod_count} pod{policy.affected_pod_count !== 1 ? "s" : ""}
        </span>
      </div>
      {Object.keys(policy.pod_selector).length > 0 && (
        <div className="flex flex-wrap gap-1">
          {Object.entries(policy.pod_selector).map(([k, v]) => (
            <span
              key={k}
              className="text-[9px] px-1.5 py-0.5 rounded bg-slate-800/80 text-slate-400 font-mono"
            >
              {k}={v}
            </span>
          ))}
        </div>
      )}
      {Object.keys(policy.pod_selector).length === 0 && (
        <span className="text-[9px] px-1.5 py-0.5 rounded bg-slate-800/80 text-slate-400 font-mono italic">
          all pods
        </span>
      )}
      <div className="flex gap-2 mt-1.5 text-[9px] text-slate-500">
        {policy.ingress_rule_count > 0 && (
          <span>
            {policy.ingress_rule_count} ingress rule{policy.ingress_rule_count > 1 ? "s" : ""}
          </span>
        )}
        {policy.egress_rule_count > 0 && (
          <span>
            {policy.egress_rule_count} egress rule{policy.egress_rule_count > 1 ? "s" : ""}
          </span>
        )}
      </div>
    </button>
  );
}
