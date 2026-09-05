import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useTelemetry, useTelemetryStale, DEFAULT_STALE_MS } from "./telemetry";
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
  beforeEach(() =>
    useTelemetry.setState({ snapshot: null, lastAt: null, staleMs: DEFAULT_STALE_MS }),
  );

  it("timestamps each frame on setSnapshot (#10)", () => {
    const before = Date.now();
    useTelemetry.getState().setSnapshot(frame());
    expect(useTelemetry.getState().snapshot).not.toBeNull();
    expect(useTelemetry.getState().lastAt ?? 0).toBeGreaterThanOrEqual(before);
  });

  it("takes the staleness window from the backend, ignoring nonsense values (#44)", () => {
    useTelemetry.getState().setStaleMs(12_000);
    expect(useTelemetry.getState().staleMs).toBe(12_000);
    for (const bad of [0, -1, Number.NaN]) {
      useTelemetry.getState().setStaleMs(bad);
      expect(useTelemetry.getState().staleMs).toBe(DEFAULT_STALE_MS);
    }
  });
});

describe("useTelemetryStale (#10)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useTelemetry.setState({ snapshot: null, lastAt: null, staleMs: DEFAULT_STALE_MS });
  });
  afterEach(() => vi.useRealTimers());

  it("is stale before any frame, live after a frame, stale again after >5s of silence", () => {
    const { result } = renderHook(() => useTelemetryStale());
    expect(result.current).toBe(true); // no frame yet

    act(() => useTelemetry.getState().setSnapshot(frame()));
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current).toBe(false); // fresh frame within the window

    act(() => vi.advanceTimersByTime(DEFAULT_STALE_MS + 1000));
    expect(result.current).toBe(true); // stream frozen
  });

  it("honours a configured window longer than the default (#44)", () => {
    useTelemetry.setState({ staleMs: 30_000 });
    const { result } = renderHook(() => useTelemetryStale());
    act(() => useTelemetry.getState().setSnapshot(frame()));
    act(() => vi.advanceTimersByTime(DEFAULT_STALE_MS + 2000));
    expect(result.current).toBe(false); // still inside the configured 30 s window
    act(() => vi.advanceTimersByTime(30_000));
    expect(result.current).toBe(true);
  });
});
