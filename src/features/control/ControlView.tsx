import { useExperiment } from "../../stores/experiment";
import { ControlPanel } from "./ControlPanel";
import { MonitorPanel } from "./MonitorPanel";
import { StageProgress } from "./StageProgress";
import { TempChart } from "./TempChart";

/** 综合控制 — the live control room (M3), now with a stage stepper while running. */
export function ControlView() {
  const running = useExperiment((s) => s.state.running);
  return (
    <main className="flex-1 min-h-0 flex flex-col gap-3 p-3">
      {running && <StageProgress />}
      <div
        className="flex-1 min-h-0 grid gap-3"
        style={{ gridTemplateColumns: "1fr 320px", gridTemplateRows: "1fr 250px" }}
      >
        <div className="min-h-0 overflow-auto">
          <MonitorPanel />
        </div>
        <div className="row-span-2 min-h-0 overflow-auto">
          <ControlPanel />
        </div>
        <div className="panel min-h-0">
          <TempChart />
        </div>
      </div>
    </main>
  );
}
