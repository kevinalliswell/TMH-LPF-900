import { describe, it, expect } from "vitest";
import { mockSetFlow } from "./mock";

// The web-preview mock mirrors the backend safety clamp (docs/08 §8.2). These guard against
// the two diverging (the mock hard-codes the same 5/5 absolute ceiling as the backend).
describe("mockSetFlow clamp", () => {
  it("clamps H2/CO above the 5.0 L/min configured limit", () => {
    const r = mockSetFlow(0, 8);
    expect(r.applied).toBe(5);
    expect(r.clamped).toBe(true);
    expect(mockSetFlow(3, 9).applied).toBe(5);
  });

  it("zero passes and invalid command inputs are rejected (#21)", () => {
    expect(mockSetFlow(0, 0).applied).toBe(0);
    expect(mockSetFlow(0, 0).clamped).toBe(false);
    expect(() => mockSetFlow(0, -3)).toThrow("INVALID_PARAMS");
    expect(() => mockSetFlow(3, Number.NaN)).toThrow("INVALID_PARAMS");
    expect(() => mockSetFlow(0, Number.POSITIVE_INFINITY)).toThrow("INVALID_PARAMS");
  });

  it("leaves N2/CO2 unlimited", () => {
    const r = mockSetFlow(1, 15);
    expect(r.applied).toBe(15);
    expect(r.clamped).toBe(false);
  });
});
