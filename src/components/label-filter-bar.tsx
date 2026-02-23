import { useState } from "react";
import { X, Filter } from "lucide-react";
import { cn } from "@/lib/utils";

interface LabelFilterBarProps {
  labels: string[];
  onLabelsChange: (labels: string[]) => void;
}

export function LabelFilterBar({ labels, onLabelsChange }: LabelFilterBarProps) {
  const [input, setInput] = useState("");

  const handleAdd = () => {
    const trimmed = input.trim();
    if (trimmed && !labels.includes(trimmed)) {
      onLabelsChange([...labels, trimmed]);
      setInput("");
    }
  };

  const handleRemove = (label: string) => {
    onLabelsChange(labels.filter((l) => l !== label));
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleAdd();
    }
    if (e.key === "Backspace" && !input && labels.length > 0) {
      onLabelsChange(labels.slice(0, -1));
    }
  };

  if (labels.length === 0 && !input) {
    return null;
  }

  return (
    <div className="flex items-center gap-2 flex-wrap">
      <Filter className="w-3.5 h-3.5 text-slate-500 shrink-0" />
      {labels.map((label) => (
        <span
          key={label}
          className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-accent/10 text-accent border border-accent/30"
        >
          <span className="font-mono">{label}</span>
          <button
            onClick={() => handleRemove(label)}
            className="hover:text-red-400 transition"
          >
            <X className="w-3 h-3" />
          </button>
        </span>
      ))}
      <input
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder="app=name"
        className={cn(
          "bg-transparent text-xs text-slate-300 outline-none placeholder:text-slate-600 w-24",
          labels.length === 0 && "w-32",
        )}
      />
    </div>
  );
}
