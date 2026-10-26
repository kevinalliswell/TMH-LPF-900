import { useEffect, useRef } from "react";
import * as echarts from "echarts";
import type { SampleDto } from "../../ipc/types";

// jsdom lacks a real 2D context; skip ECharts there so unit tests don't crash.
function canRenderCanvas(): boolean {
  try {
    const c = document.createElement("canvas");
    return !!(c.getContext && c.getContext("2d"));
  } catch {
    return false;
  }
}

function sampleHasValidDeviceReading(
  sample: SampleDto,
  key: "temp_valid" | "balance_valid",
): boolean {
  const object = (value: unknown): Record<string, unknown> | null =>
    typeof value === "object" && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  const record = object(sample.record);
  const telemetry = object(record?.["telemetry"]);
  const health = object(telemetry?.["health"]);
  return health?.[key] !== false;
}

/** Static temperature + mass replay of a stored experiment (dual Y axis). */
export function HistoryChart({ samples }: { samples: SampleDto[] }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!ref.current || !canRenderCanvas() || samples.length === 0) return;
    let chart: echarts.ECharts;
    try {
      chart = echarts.init(ref.current, undefined, { renderer: "canvas" });
    } catch {
      return;
    }
    chart.setOption({
      animation: false,
      grid: { left: 50, right: 50, top: 24, bottom: 24 },
      textStyle: { color: "#8b98a5", fontFamily: "ui-monospace, monospace" },
      tooltip: { trigger: "axis" },
      legend: { data: ["温度 °C", "质量 g"], textStyle: { color: "#8b98a5" }, top: 0, right: 8 },
      xAxis: {
        type: "category",
        data: samples.map((s) => s.experiment_duration ?? ""),
        axisLine: { lineStyle: { color: "#1d2733" } },
        axisLabel: { color: "#566171", fontSize: 10, showMaxLabel: true },
      },
      yAxis: [
        {
          type: "value",
          name: "°C",
          min: 0,
          max: 1000,
          splitLine: { lineStyle: { color: "#141c26" } },
          axisLabel: { color: "#566171", fontSize: 10 },
        },
        {
          type: "value",
          name: "g",
          splitLine: { show: false },
          axisLabel: { color: "#566171", fontSize: 10 },
        },
      ],
      series: [
        {
          name: "温度 °C",
          type: "line",
          showSymbol: false,
          yAxisIndex: 0,
          lineStyle: { width: 2, color: "#ff8c42" },
          data: samples.map((s) => sampleHasValidDeviceReading(s, "temp_valid") ? s.temperature : null),
        },
        {
          name: "质量 g",
          type: "line",
          showSymbol: false,
          yAxisIndex: 1,
          lineStyle: { width: 2, color: "#38d6c4" },
          data: samples.map((s) => sampleHasValidDeviceReading(s, "balance_valid") ? s.weight : null),
        },
      ],
    });
    const ro = new ResizeObserver(() => chart.resize());
    ro.observe(ref.current);
    return () => {
      ro.disconnect();
      chart.dispose();
    };
  }, [samples]);

  return <div ref={ref} className="w-full" style={{ height: 260 }} />;
}
