import { describe, it, expect, beforeEach } from "vitest";
import { toastError, useToasts } from "./toast";

describe("toastError (error code → Chinese, docs/08 §8.2)", () => {
  beforeEach(() => useToasts.setState({ toasts: [] }));

  it("maps a known error code to its Chinese message", () => {
    toastError("PORT_IN_USE");
    const t = useToasts.getState().toasts;
    expect(t).toHaveLength(1);
    expect(t[0]?.kind).toBe("error");
    expect(t[0]?.msg).toContain("串口被其他程序占用");
  });

  it("maps INVALID_PARAMS", () => {
    toastError("INVALID_PARAMS");
    expect(useToasts.getState().toasts.at(-1)?.msg).toContain("参数校验失败");
  });

  it("extracts the message from an Error and passes unmapped text through", () => {
    toastError(new Error("device offline: mfc"));
    expect(useToasts.getState().toasts.at(-1)?.msg).toBe("device offline: mfc");
  });
});
