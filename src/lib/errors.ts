/**
 * Format an error into a user-friendly string.
 * Replaces all uses of alert() with a consistent error pattern.
 */
export function formatError(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err ?? "Unknown error");
}
