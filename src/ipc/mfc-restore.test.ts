import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TelemetrySnapshot } from "./types";

beforeEach(() => vi.resetModules());

async function devices() {
  const mock = await import("./mock");
  const snapshot = () => {
    let value: TelemetrySnapshot | undefined;
    const stop = mock.mockTelemetry((next) => { value = next; });
    stop();
    return value!;
  };
  return { ...mock, snapshot };
}

describe("MFC ON restores acknowledged nonzero flow (#28)", () => {
  it("restores each channel after OFF, retaining the clamped value", async () => {
    const mock = await devices();
    for (const [gas, requested, applied] of [[0, 8, 5], [1, 15, 15], [2, 3, 3], [3, 2, 2]]) {
      mock.mockSetFlow(gas!, requested!);
      mock.mockSetOnOff(gas!, false);
      expect(mock.snapshot().gas[gas!]!.sv).toBe(0);
      mock.mockSetOnOff(gas!, true);
      expect(mock.snapshot().gas[gas!]).toMatchObject({ sv: applied, on: true });
    }
  });

  it("rejects ON without a previously confirmed nonzero value", async () => {
    const mock = await devices();
    expect(() => mock.mockSetOnOff(0, true)).toThrow(/可恢复/);
    mock.mockSetFlow(1, 0);
    expect(() => mock.mockSetOnOff(1, true)).toThrow(/可恢复/);
    expect(mock.snapshot().gas.every((gas) => gas.sv === 0 && !gas.on)).toBe(true);
  });

  it("reports a failed bulk open instead of success when one channel cannot resume", async () => {
    const mock = await devices();
    const { openAllValves } = await import("./index");
    mock.mockSetFlow(0, 3);
    mock.mockSetOnOff(0, false);
    await expect(openAllValves()).rejects.toThrow(/可恢复/);
    expect(mock.snapshot().gas[1]!.sv).toBe(0);
  });

  it("does not treat a request below the wire resolution as confirmed nonzero flow", async () => {
    const mock = await devices();
    expect(mock.mockSetFlow(0, 0.001).applied).toBe(0); // H2 resolves to 0.01 L/min wire units.
    expect(mock.snapshot().gas[0]).toMatchObject({ sv: 0, on: false });
    mock.mockSetOnOff(0, false);
    expect(() => mock.mockSetOnOff(0, true)).toThrow(/可恢复/);
  });

  it("reports the confirmed wire value and keeps ordinary decimal boundaries", async () => {
    const mock = await devices();
    for (const [request, applied] of [[1.234, 1.23], [1.235, 1.24], [0.3, 0.3], [1.5, 1.5]]) {
      expect(mock.mockSetFlow(0, request!).applied).toBe(applied);
      expect(mock.snapshot().gas[0]!.sv).toBe(applied);
    }
  });

  it("rejects an unrepresentable flow without changing SV, while zero remains available", async () => {
    const mock = await devices();
    mock.mockSetFlow(1, 3);
    expect(() => mock.mockSetFlow(1, 1e308)).toThrow();
    expect(mock.snapshot().gas[1]!.sv).toBe(3);
    expect(mock.mockSetFlow(1, 0).applied).toBe(0);
  });

  it("attempts later bulk channels after a missing restore value and reports partial failure", async () => {
    const mock = await devices();
    mock.mockSetFlow(1, 3);
    mock.mockSetOnOff(1, false);
    mock.mockSetFlow(3, 2);
    mock.mockSetOnOff(3, false);
    expect(() => mock.mockSetAllValves(true)).toThrow(/可恢复/);
    expect(mock.snapshot().gas.map((gas) => gas.sv)).toEqual([0, 3, 0, 2]);
    mock.mockSetAllValves(false);
    expect(mock.snapshot().gas.every((gas) => gas.sv === 0)).toBe(true);
  });

  it("rejects every ON while protection remains latched", async () => {
    const mock = await devices();
    for (let gas = 0; gas < 4; gas += 1) mock.mockSetFlow(gas, 3);
    mock.mockShutoff();
    for (let gas = 0; gas < 4; gas += 1) {
      expect(() => mock.mockSetOnOff(gas, true)).toThrow(/锁存/);
    }
    expect(mock.snapshot().gas.map((gas) => gas.sv)).toEqual([0, 5, 0, 0]);
  });
});

it("rejects invalid manual flow rather than silently closing a channel", async () => {
  const mock = await devices();
  mock.mockSetFlow(0, 2);
  for (const value of [-1, Number.NaN, Number.POSITIVE_INFINITY]) {
    expect(() => mock.mockSetFlow(0, value)).toThrow("INVALID_PARAMS");
    expect(mock.snapshot().gas[0]!.sv).toBe(2);
  }
});
