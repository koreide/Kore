import { useEffect, useMemo, useState } from "react";
import * as Command from "cmdk";

interface CommandPaletteProps {
  open: boolean;
  contexts: string[];
  onClose: () => void;
  onSelectContext: (context: string) => void;
}

export function CommandPalette({ open, contexts, onClose, onSelectContext }: CommandPaletteProps) {
  const [search, setSearch] = useState("");

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const filtered = useMemo(
    () => contexts.filter((c) => c.toLowerCase().includes(search.toLowerCase())),
    [contexts, search]
  );

  if (!open) return null;

  return (
    <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-start justify-center pt-24 z-50">
      <Command.Command className="w-full max-w-xl rounded-lg border border-slate-800 glass shadow-xl">
        <Command.CommandInput
          autoFocus
          placeholder="Switch context..."
          value={search}
          onValueChange={setSearch}
          className="w-full px-4 py-3 bg-transparent outline-none text-slate-100"
        />
        <Command.CommandList>
          {filtered.map((ctx) => (
            <Command.CommandItem
              key={ctx}
              onSelect={() => {
                onSelectContext(ctx);
                onClose();
              }}
              className="px-4 py-2 cursor-pointer hover:bg-muted/60"
            >
              {ctx}
            </Command.CommandItem>
          ))}
          {filtered.length === 0 && (
            <div className="px-4 py-3 text-slate-500 text-sm">No matching contexts</div>
          )}
        </Command.CommandList>
      </Command.Command>
    </div>
  );
}


