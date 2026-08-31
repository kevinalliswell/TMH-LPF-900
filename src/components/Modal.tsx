import { useEffect } from "react";
import { X } from "lucide-react";

export function Modal({
  title,
  onClose,
  children,
  wide = false,
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  wide?: boolean;
}) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: "rgba(3,6,10,0.62)", backdropFilter: "blur(2px)" }}
      onClick={onClose}
    >
      <div
        className={`panel p-4 max-w-[92vw] max-h-[90vh] overflow-auto ${wide ? "w-[980px]" : "w-[460px]"}`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-3">
          <div className="text-sm font-semibold">{title}</div>
          <button onClick={onClose} style={{ color: "var(--fg-faint)" }} aria-label="关闭">
            <X size={16} aria-hidden="true" />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
