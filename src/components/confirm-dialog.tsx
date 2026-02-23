import { motion, AnimatePresence } from "framer-motion";
import { AlertTriangle, X } from "lucide-react";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  onConfirm: () => void;
  onCancel: () => void;
  variant?: "danger" | "warning" | "info";
}

export function ConfirmDialog({
  open,
  title,
  message,
  confirmText = "Confirm",
  cancelText = "Cancel",
  onConfirm,
  onCancel,
  variant = "danger",
}: ConfirmDialogProps) {
  if (!open) return null;

  const variantStyles = {
    danger: "border-red-500/50 bg-red-500/10",
    warning: "border-yellow-500/50 bg-yellow-500/10",
    info: "border-blue-500/50 bg-blue-500/10",
  };

  const buttonStyles = {
    danger: "bg-red-600 hover:bg-red-700 border-red-500",
    warning: "bg-yellow-600 hover:bg-yellow-700 border-yellow-500",
    info: "bg-blue-600 hover:bg-blue-700 border-blue-500",
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
            className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50"
            onClick={onCancel}
          />

          {/* Dialog */}
          <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
            <motion.div
              role="dialog"
              aria-modal="true"
              aria-label={title}
              initial={{ opacity: 0, scale: 0.95, y: 20 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.95, y: 20 }}
              className={`bg-surface border rounded-lg shadow-xl max-w-md w-full ${variantStyles[variant]}`}
              onClick={(e) => e.stopPropagation()}
            >
              <div className="p-6">
                <div className="flex items-start gap-4 mb-4">
                  <div
                    className={`p-2 rounded-full ${
                      variant === "danger"
                        ? "bg-red-500/20"
                        : variant === "warning"
                          ? "bg-yellow-500/20"
                          : "bg-blue-500/20"
                    }`}
                  >
                    <AlertTriangle
                      className={`w-6 h-6 ${
                        variant === "danger"
                          ? "text-red-400"
                          : variant === "warning"
                            ? "text-yellow-400"
                            : "text-blue-400"
                      }`}
                    />
                  </div>
                  <div className="flex-1">
                    <h3 className="text-lg font-semibold text-slate-100 mb-2">{title}</h3>
                    <p className="text-slate-300 text-sm">{message}</p>
                  </div>
                  <button
                    onClick={onCancel}
                    className="text-slate-400 hover:text-slate-200 transition"
                  >
                    <X className="w-5 h-5" />
                  </button>
                </div>

                <div className="flex gap-3 justify-end mt-6">
                  <button
                    onClick={onCancel}
                    className="px-4 py-2 rounded border border-slate-800 hover:border-slate-700 hover:bg-muted/40 transition text-sm"
                  >
                    {cancelText}
                  </button>
                  <button
                    onClick={onConfirm}
                    className={`px-4 py-2 rounded border transition text-sm text-white ${buttonStyles[variant]}`}
                  >
                    {confirmText}
                  </button>
                </div>
              </div>
            </motion.div>
          </div>
        </>
      )}
    </AnimatePresence>
  );
}
