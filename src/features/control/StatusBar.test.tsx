import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useTelemetry } from "../../stores/telemetry";
import { StatusBar } from "./StatusBar";
import { HomeView } from "../home/HomeView";

afterEach(cleanup);
it("uses the applied freshness timeout in the stale warning and home readings", async () => {
  useTelemetry.setState({staleMs: 2000, lastAt: Date.now() - 3000, snapshot: {
    seq: 1, temps: Array(9).fill(321.5), weight: 99.99, gas: [],
    health: {temp_connected: true, mfc_connected: true, balance_connected: true, temp_valid: true, mfc_valid: true, balance_valid: true},
  }});
  await act(async () => { render(<><StatusBar /><HomeView /></>); });
  expect(screen.getByText("数据陈旧（>2s）")).toBeTruthy();
  expect(screen.queryByText("321.5")).toBeNull();
  expect(screen.queryByText("99.99")).toBeNull();
  expect(screen.getByText("数据陈旧，读值暂不可用")).toBeTruthy();
});

it("hides invalid device values even while fresh telemetry frames continue arriving", async () => {
  useTelemetry.setState({staleMs: 5000, lastAt: Date.now(), snapshot: {
    seq: 2, temps: Array(9).fill(321.5), weight: 99.99, gas: [],
    health: {temp_connected: true, mfc_connected: true, balance_connected: true, temp_valid: false, mfc_valid: true, balance_valid: false},
  }});
  await act(async () => { render(<HomeView />); });
  expect(screen.queryByText("321.5")).toBeNull();
  expect(screen.queryByText("99.99")).toBeNull();
  expect(screen.getByText("温度读值不可用")).toBeTruthy();
  expect(screen.getByText("天平读值不可用")).toBeTruthy();
});
