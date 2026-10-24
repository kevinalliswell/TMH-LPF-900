import { describe, expect, it } from "vitest";
import { CommandFailure, normalizeCommandError } from "./errors";

describe("command errors", () => {
  it("keeps native codes and localized messages readable by existing UI consumers", () => {
    const error = normalizeCommandError({ code: "FORBIDDEN", message_zh: "当前账户无权执行此操作", detail: null });
    expect(error).toBeInstanceOf(CommandFailure);
    expect((error as CommandFailure).code).toBe("FORBIDDEN");
    expect(error.message).toBe("当前账户无权执行此操作");
    expect(String(error)).toBe(error.message);
    expect(String(error)).not.toContain("[object Object]");
  });

  it("preserves framework string errors and browser mock Error instances", () => {
    expect(normalizeCommandError("missing required key id").message).toBe("missing required key id");
    const error = new Error("AUTH_REQUIRED");
    expect(normalizeCommandError(error)).toBe(error);
  });

  it("does not render an unrecognized response or diagnostic detail", () => {
    const error = normalizeCommandError({ code: "STORAGE_ERROR", message_zh: "存储失败", detail: "internal" });
    expect(error.message).toBe("存储失败");
    expect(normalizeCommandError({ message_zh: 123 }).message).not.toContain("object");
  });
});
