import { useState } from "react";
import type { ExperimentSummary, GroupRequest, GroupReportDetails, MaterialKind, TestGroupRevision } from "../../ipc";
const inputClass = "w-full p-2 rounded border border-[var(--border)] bg-[var(--panel-2)] text-[13px]";
const reportFields: [keyof GroupReportDetails,string][] = [
  ["lab_name","实验室"],["lab_address","实验室地址"],["report_number","报告编号"],
  ["report_date","报告日期"],["reviewer","复核人（人工录入）"],["deviations","偏离及处理说明"],
];
export function GroupForm({value,change,experiments,record,disabled}: {
  value:GroupRequest;change:(value:GroupRequest)=>void;experiments:ExperimentSummary[];
  record:TestGroupRevision|null;disabled:boolean;
}) {
  const [search,setSearch]=useState("");
  const retained = new Set(record?.snapshot.trials.map(t=>t.experiment.experiment_id));
  const picked = new Set(value.trials.map(t=>t.experiment_id));
  const candidates = experiments.filter(e=>!picked.has(e.experiment_id) &&
    `${e.sample_id ?? ""} ${e.sample_name} ${e.experiment_id}`.toLowerCase().includes(search.toLowerCase())).slice(0,50);
  function changeTrial(id:string,reason:string|null) {
    change({...value,trials:value.trials.map(t=>t.experiment_id===id?{...t,exclusion_reason:reason}:t)});
  }
  return <fieldset disabled={disabled} className="space-y-4 disabled:opacity-60">
    <div className="panel p-4 space-y-3">
      <h2 className="font-semibold">1. 批次与检验依据</h2>
      <div className="grid grid-cols-2 gap-3">
        <label>检验组名称<input className={inputClass} value={value.name} disabled={!!record} maxLength={4096} onChange={e=>change({...value,name:e.target.value})}/></label>
        <label>批次 / 试样标识<input className={inputClass} value={value.batch_id} disabled={!!record} maxLength={4096} onChange={e=>change({...value,batch_id:e.target.value})}/></label>
        <label>材料类别<select className={inputClass} value={value.material} disabled={!!record} onChange={e=>change({...value,material:e.target.value as MaterialKind})}>
          <option value="pellet">球团矿</option><option value="sinter">烧结矿</option><option value="natural">天然块矿</option>
        </select></label>
      </div>
      <label className="flex gap-2 items-center"><input type="checkbox" checked={value.same_batch_confirmed} onChange={e=>change({...value,same_batch_confirmed:e.target.checked})}/>确认来自同一批试样的独立试验</label>
      <label className="flex gap-2 items-center"><input type="checkbox" checked={!!value.manual_limits} disabled={!!record} onChange={e=>change({...value,manual_limits:e.target.checked?{a:NaN,b:NaN,c:NaN,basis:""}:null})}/>人工填写重复性限值及依据</label>
      <p className="text-[12px] text-[var(--fg-dim)]">天然块矿还原性须填写供需双方商定限值；低温粉化须按标准表 1 核定并填写依据。其余方法使用固定限值，不能人工覆盖。</p>
      {value.manual_limits && <div className="grid grid-cols-3 gap-3">
        {(["a","b","c"] as const).map(key=><label key={key}>{key.toUpperCase()}（百分点）<input className={inputClass} type="number" min="0" step="any" disabled={!!record} value={Number.isFinite(value.manual_limits![key])?value.manual_limits![key]:""} onChange={e=>change({...value,manual_limits:{...value.manual_limits!,[key]:e.target.valueAsNumber}})}/></label>)}
        <label className="col-span-3">限值来源与确认依据<textarea className={inputClass} value={value.manual_limits.basis} disabled={!!record} maxLength={4096} onChange={e=>change({...value,manual_limits:{...value.manual_limits!,basis:e.target.value}})}/></label>
      </div>}
    </div>
    <div className="panel p-4 space-y-3">
      <h2 className="font-semibold">2. 选择独立试验</h2>
      <p className="text-[12px] text-[var(--fg-dim)]">先在历史查询完成单次分析和报告补录。有效记录最多 4 次，系统按开始时间排序；已保存成员不能移除，可填写具体原因排除。</p>
      {value.trials.map(t=>{
        const frozen=record?.snapshot.trials.find(f=>f.experiment.experiment_id===t.experiment_id)?.experiment;
        const e=frozen??experiments.find(e=>e.experiment_id===t.experiment_id);
        return <div key={t.experiment_id} className="p-3 border border-[var(--border)] rounded space-y-2">
          <div className="flex justify-between gap-3"><span>{e?.sample_id??t.experiment_id} · {e?.sample_name??"已保存证据"}</span>
            {!retained.has(t.experiment_id) && <button type="button" onClick={()=>change({...value,trials:value.trials.filter(v=>v!==t)})}>移除</button>}</div>
          <div className="text-[11px] text-[var(--fg-dim)] break-all">{t.experiment_id} · {e?.start_time}</div>
          <label className="flex items-center gap-2"><input type="checkbox" checked={t.exclusion_reason!==null} onChange={e=>changeTrial(t.experiment_id,e.target.checked?"":null)}/>人工排除 {e?.sample_id??t.experiment_id}</label>
          {t.exclusion_reason!==null && <label>排除原因<textarea className={inputClass} maxLength={4096} value={t.exclusion_reason} onChange={e=>changeTrial(t.experiment_id,e.target.value)}/></label>}
        </div>;
      })}
      <label>查找试验<input className={inputClass} value={search} onChange={e=>setSearch(e.target.value)} placeholder="样品编号、名称或试验编号"/></label>
      <div className="max-h-56 overflow-auto border border-[var(--border)] rounded divide-y divide-[var(--border)]">
        {candidates.map(e=><label key={e.experiment_id} className="flex gap-3 items-center p-2"><input aria-label={`选择 ${e.sample_id??e.experiment_id}`} type="checkbox" checked={false} disabled={value.trials.length>=20} onChange={()=>change({...value,trials:[...value.trials,{experiment_id:e.experiment_id,exclusion_reason:null}]})}/><span>{e.sample_id??e.experiment_id} · {e.sample_name}<small className="block text-[var(--fg-dim)]">{e.start_time} · {e.has_analysis?"已分析":"未分析"} · {e.end_time?"已结束":"未结束"}</small></span></label>)}
        {candidates.length===0 && <p className="p-3 text-[var(--fg-dim)]">没有匹配的未选试验</p>}
      </div>
      <p className="text-[11px] text-[var(--fg-dim)]">最多显示 50 个匹配项，可输入编号进一步查找。已选 {value.trials.length} 项。</p>
    </div>
    <div className="panel p-4 space-y-3">
      <h2 className="font-semibold">3. 报告信息与保存说明</h2>
      <div className="grid grid-cols-2 gap-3">{reportFields.map(([key,label])=><label key={key}>{label}<textarea className={inputClass} maxLength={4096} value={value.report_details[key]} onChange={e=>change({...value,report_details:{...value.report_details,[key]:e.target.value}})}/></label>)}</div>
      <p className="text-[12px] text-[var(--fg-dim)]">留空的报告资料标为“待补录”。制样、设备、校准及外部操作等资料在各单次试验中补录，并随组版本保留。</p>
      <label className="block">本次保存原因<textarea className={inputClass} maxLength={4096} value={value.reason} onChange={e=>change({...value,reason:e.target.value})}/></label>
    </div>
  </fieldset>;
}
