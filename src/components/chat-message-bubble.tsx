import { Bot, Copy } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "@/lib/utils";

interface ChatMessageBubbleProps {
  role: "user" | "assistant";
  content: string;
  onCopy?: (content: string) => void;
  variant?: "compact" | "comfortable";
}

const styles = {
  compact: {
    gap: "gap-2.5",
    avatar: "w-6 h-6 rounded-md",
    icon: "w-3.5 h-3.5",
    bubble: "rounded-lg px-3 py-2",
    assistantBg: "bg-muted/60 text-slate-200 border border-slate-800/50 rounded-bl-sm",
  },
  comfortable: {
    gap: "gap-3",
    avatar: "w-7 h-7 rounded-lg",
    icon: "w-4 h-4",
    bubble: "rounded-xl px-4 py-3",
    assistantBg: "bg-surface border border-slate-800/50 text-slate-200 rounded-bl-sm",
  },
};

export function ChatMessageBubble({
  role,
  content,
  onCopy,
  variant = "compact",
}: ChatMessageBubbleProps) {
  const s = styles[variant];

  return (
    <div className={cn("flex", s.gap, role === "user" ? "justify-end" : "justify-start")}>
      {role === "assistant" && (
        <div
          className={cn(s.avatar, "bg-accent/15 flex items-center justify-center shrink-0 mt-0.5")}
        >
          <Bot className={cn(s.icon, "text-accent")} />
        </div>
      )}

      <div
        className={cn(
          "relative group max-w-[85%] text-sm leading-relaxed overflow-hidden",
          s.bubble,
          role === "user" ? "bg-accent/15 text-slate-100 rounded-br-sm" : s.assistantBg,
        )}
      >
        {role === "user" ? (
          <p className="whitespace-pre-wrap break-words">{content}</p>
        ) : (
          <div className="ai-markdown">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
          </div>
        )}

        {role === "assistant" && onCopy && (
          <button
            onClick={() => onCopy(content)}
            className="absolute -top-2 -right-2 p-1 rounded bg-surface border border-slate-700 opacity-0 group-hover:opacity-100 transition-opacity hover:border-accent/50"
            aria-label="Copy message"
          >
            <Copy className="w-3 h-3 text-slate-400" />
          </button>
        )}
      </div>
    </div>
  );
}
