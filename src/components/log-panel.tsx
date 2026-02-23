interface LogPanelProps {
  logs: string;
}

export function LogPanel({ logs }: LogPanelProps) {
  return (
    <div className="h-48 border border-slate-800 rounded-lg glass overflow-auto text-xs leading-relaxed px-3 py-2 font-mono bg-black/40">
      <pre className="whitespace-pre-wrap">{logs || ""}</pre>
    </div>
  );
}
