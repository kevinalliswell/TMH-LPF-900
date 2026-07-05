import { useEffect, useRef } from "react";
import * as echarts from "echarts";
import { useTelemetry } from "../../stores/telemetry";

const COLORS = [
  "#ff8c42", "#ffa15c", "#e8734a", "#ffb877", "#ff6b3d", "#f59e6b", "#ffce8f",
  "#ff8c42", "#d9633a",
];

// jsdom lacks a real 2D context; skip ECharts there so unit tests don't crash.
function canRenderCanvas(): boolean {
  try {
    const c = document.createElement("canvas");
    return !!(c.getContext && c.getContext("2d"));
  } catch {
    return false;
  }
}

export function TempChart() {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!ref.current || !canRenderCanvas()) return;
    let chart: echarts.ECharts;
    try {
      chart = echarts.init(ref.current, undefined, { renderer: "canvas" });
    } catch {
      return;
    }

    const MAX = 600; // ~2 min at 5 Hz; ring buffer, drop oldest
    const xs: number[] = [];
    const series: number[][] = Array.from({ length: 9 }, () => []);

    chart.setOption({
      animation: false,
      grid: { left: 46, right: 14, top: 12, bottom: 22 },
      textStyle: { color: "#8b98a5", fontFamily: "ui-monospace, monospace" },
      tooltip: { show: false },
      xAxis: {
        type: "category",
        data: xs,
        axisLine: { lineStyle: { color: "#1d2733" } },
        axisLabel: { color: "#566171", fontSize: 10 },
      },
      yAxis: {
        type: "value",
        min: 0,
        max: 1000,
        splitLine: { lineStyle: { color: "#141c26" } },
        axisLabel: { color: "#566171", fontSize: 10 },
      },
      series: series.map((d, i) => ({
        name: `T${i + 1}`,
        type: "line",
        showSymbol: false,
        emphasis: { disabled: true },
        lineStyle: { width: i === 7 ? 2.2 : 1, color: i === 7 ? "#ff8c42" : COLORS[i] },
        data: d,
      })),
    });

    let x = 0;
    const unsub = useTelemetry.subscribe((state) => {
      const snap = state.snapshot;
      if (!snap) return;
      // Time axis advances exactly once per frame (docs/07).
      xs.push(x);
      x += 1;
      snap.temps.forEach((t, i) => series[i]?.push(t ?? 0));
      if (xs.length > MAX) {
        xs.shift();
        series.forEach((s) => s.shift());
      }
      chart.setOption({ xAxis: { data: xs }, series: series.map((d) => ({ data: d })) });
    });

    const ro = new ResizeObserver(() => chart.resize());
    ro.observe(ref.current);
    return () => {
      unsub();
      ro.disconnect();
      chart.dispose();
    };
  }, []);

  return (
    <div className="flex flex-col h-full p-2">
      <div className="section-title px-1 pb-1">温度曲线 · °C（T8 样品加粗）</div>
      <div ref={ref} className="flex-1 min-h-0" />
    </div>
  );
}
