// Preserve the acknowledged wire value while hiding floating-point arithmetic noise.
const flowNumber = new Intl.NumberFormat("zh-CN", {
  useGrouping: false,
  maximumSignificantDigits: 12,
});
export function formatFlow(value: number): string {
  return Number.isFinite(value) ? flowNumber.format(value) : "—";
}
