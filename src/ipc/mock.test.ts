import { describe, it, expect } from "vitest";
import {
  mockChangeAdminPassword,
  mockGenerateSampleId,
  mockGetAdminAuthStatus,
  mockGetCommConfig,
  mockGetExperimentStartDefaults,
  mockSetFlow,
  mockUpdateCommConfig,
} from "./mock";

// The web-preview mock mirrors the backend safety clamp (docs/08 §8.2). These guard against
// the two diverging (the mock hard-codes the same 10/10 absolute ceiling as the backend).
describe("mockSetFlow clamp", () => {
  it("clamps H2/CO above the 5.0 L/min configured limit", () => {
    const r = mockSetFlow(0, 8);
    expect(r.applied).toBe(5);
    expect(r.clamped).toBe(true);
    expect(mockSetFlow(3, 9).applied).toBe(5);
  });

  it("zero passes and negatives / non-finite floor to 0 (#4)", () => {
    expect(mockSetFlow(0, 0).applied).toBe(0);
    expect(mockSetFlow(0, 0).clamped).toBe(false);
    expect(mockSetFlow(0, -3).applied).toBe(0);
    expect(mockSetFlow(3, Number.NaN).applied).toBe(0);
    expect(mockSetFlow(0, Number.POSITIVE_INFINITY).applied).toBe(0);
  });

  it("leaves N2/CO2 unlimited", () => {
    const r = mockSetFlow(1, 15);
    expect(r.applied).toBe(15);
    expect(r.clamped).toBe(false);
  });
});

describe("mock experiment setup", () => {
  it("generates sample ids from mode prefix, date, and sequence", () => {
    expect(mockGenerateSampleId("LOW_TEMP_DEGRADATION", "2026-08-31T22:16")).toBe("RDI-20260831-001");
  });

  it("loads start defaults from the latest experiment and current balance", () => {
    const defaults = mockGetExperimentStartDefaults("REDUCIBILITY");
    expect(defaults.sample_id).toMatch(/^RED-\d{8}-\d{3}$/);
    expect(defaults.sample_name).toBeTruthy();
    expect(defaults.operator).toBeTruthy();
    expect(defaults.sample_weight).toBeGreaterThan(0);
  });

  it("requires changing the default admin password before saving comm config", () => {
    expect(mockGetAdminAuthStatus().must_change).toBe(true);
    expect(() => mockUpdateCommConfig(mockGetCommConfig(), "1952")).toThrow("请先修改默认管理员口令");
    mockChangeAdminPassword("1952", "2468");
    const cfg = mockGetCommConfig();
    cfg.COM_RS485_MFC.port = "COM9";
    expect(mockUpdateCommConfig(cfg, "2468").COM_RS485_MFC.port).toBe("COM9");
  });
});
