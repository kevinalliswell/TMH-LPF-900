import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useTelemetry, useTelemetryStale, STALE_MS } from "./telemetry";
import type { TelemetrySnapshot } from "../ipc/types";

const frame = (): TelemetrySnapshot =>
  ({
    seq: 1,
    temps: Array(9).fill(null),
    gas: [],
    weight: null,
    health: { temp_connected: true, mfc_connected: true, balance_connected: true },
  }) as unknown as TelemetrySnapshot;

describe("telemetry store", () => {
  beforeEach(() => useTelemetry.setState({ snapshot: null, lastAt: null }));

  it("timestamps each frame on setSnapshot (#10)", () => {
    const before = Date.now();
    useTelemetry.getState().setSnapshot(frame());
    expect(useTelemetry.getState().snapshot).not.toBeNull();
    expect(useTelemetry.getState().lastAt ?? 0).toBeGreaterThanOrEqual(before);
  });
});

describe("useTelemetryStale (#10)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useTelemetry.setState({ snapshot: null, lastAt: null });
  });
  afterEach(() => vi.useRealTimers());

  it("is stale before any frame, live after a frame, stale again after >5s of silence", () => {
    const { result } = renderHook(() => useTelemetryStale());
    expect(result.current).toBe(true); // no frame yet

    act(() => useTelemetry.getState().setSnapshot(frame()));
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current).toBe(false); // fresh frame within the window

    act(() => vi.advanceTimersByTime(STALE_MS + 1000));
    expect(result.current).toBe(true); // stream frozen
  });
});
