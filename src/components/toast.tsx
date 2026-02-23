import { createContext, useCallback, useContext, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { CheckCircle2, XCircle, Info, X } from "lucide-react";
import { cn } from "@/lib/utils";

type ToastType = "success" | "error" | "info";

interface Toast {
  id: number;
  message: string;
  type: ToastType;
}

interface ToastContextValue {
  toast: (message: string, type?: ToastType) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

let nextId = 0;

const typeStyles: Record<
  ToastType,
  { icon: typeof CheckCircle2; border: string; iconColor: string }
> = {
  success: { icon: CheckCircle2, border: "border-emerald-500/30", iconColor: "text-emerald-400" },
  error: { icon: XCircle, border: "border-red-500/30", iconColor: "text-red-400" },
  info: { icon: Info, border: "border-accent/30", iconColor: "text-accent" },
};

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const addToast = useCallback((message: string, type: ToastType = "info") => {
    const id = nextId++;
    setToasts((prev) => [...prev, { id, message, type }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 3000);
  }, []);

  const removeToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  return (
    <ToastContext.Provider value={{ toast: addToast }}>
      {children}
      <div className="fixed bottom-4 right-4 z-[9998] flex flex-col gap-2 pointer-events-none">
        <AnimatePresence>
          {toasts.map((t) => {
            const style = typeStyles[t.type];
            const Icon = style.icon;
            return (
              <motion.div
                key={t.id}
                initial={{ opacity: 0, y: 16, scale: 0.95 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 8, scale: 0.95 }}
                transition={{ duration: 0.2, ease: "easeOut" }}
                onClick={() => removeToast(t.id)}
                className={cn(
                  "pointer-events-auto cursor-pointer flex items-center gap-2.5 px-4 py-2.5 rounded-lg glass border text-sm text-slate-200 shadow-xl max-w-xs",
                  style.border,
                )}
              >
                <Icon className={cn("w-4 h-4 shrink-0", style.iconColor)} />
                <span className="flex-1">{t.message}</span>
                <X className="w-3.5 h-3.5 text-slate-500 shrink-0" />
              </motion.div>
            );
          })}
        </AnimatePresence>
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): (message: string, type?: ToastType) => void {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used within ToastProvider");
  return ctx.toast;
}
