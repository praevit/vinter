import type { CommandError } from "./bindings";

/** Turns a rejected command (a `CommandError`) or any other throwable into text. */
export function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as CommandError).message);
  }
  return String(error);
}
