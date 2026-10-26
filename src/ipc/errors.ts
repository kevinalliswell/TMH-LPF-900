/** The native application error contract; malformed invoke payloads remain Tauri errors. */
import type { CommandError } from "./types.generated";
export type { CommandError } from "./types.generated";

export class CommandFailure extends Error {
  readonly code: string;
  readonly detail: string | null;

  constructor(error: CommandError) {
    super(error.message_zh);
    this.name = "CommandFailure";
    this.code = error.code;
    this.detail = error.detail;
  }

  override toString(): string {
    return this.message;
  }
}

export function normalizeCommandError(cause: unknown): Error {
  if (cause instanceof Error) return cause;
  if (typeof cause === "object" && cause !== null &&
      "code" in cause && typeof cause.code === "string" &&
      "message_zh" in cause && typeof cause.message_zh === "string" &&
      "detail" in cause && (cause.detail === null || typeof cause.detail === "string")) {
    return new CommandFailure({ code: cause.code, message_zh: cause.message_zh, detail: cause.detail });
  }
  return new Error(typeof cause === "string" ? cause : "命令响应无效，请检查应用版本和日志");
}
