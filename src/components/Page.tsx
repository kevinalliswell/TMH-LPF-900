import type { ReactNode } from "react";

/** Scrollable, centered content container shared by the non-control pages. */
export function Page({ children, wide }: { children: ReactNode; wide?: boolean }) {
  return (
    <div className="flex-1 min-h-0 overflow-auto p-5">
      <div className={`mx-auto flex flex-col gap-4 ${wide ? "max-w-6xl" : "max-w-4xl"}`}>
        {children}
      </div>
    </div>
  );
}

/** Small caps section heading. */
export function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <div
      className="text-[11px] uppercase tracking-wider mb-2"
      style={{ color: "var(--fg-faint)" }}
    >
      {children}
    </div>
  );
}
