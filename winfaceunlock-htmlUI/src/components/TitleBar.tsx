import { Shield, Minus, Square, X } from 'lucide-react';

export function TitleBar() {
  return (
    <div className="relative z-50 flex h-12 w-full select-none items-center justify-between px-4 text-slate-700">
      <div className="flex items-center gap-2">
        <Shield className="h-4 w-4 text-[#0066b8]" strokeWidth={2.5} />
        <span className="text-sm font-medium tracking-wide">WinFaceUnlock</span>
      </div>
      <div className="flex items-center gap-5 text-slate-500">
        <button className="flex items-center justify-center transition-colors hover:text-slate-900 focus:outline-none">
          <Minus className="h-4 w-4" />
        </button>
        <button className="flex items-center justify-center transition-colors hover:text-slate-900 focus:outline-none">
          <Square className="h-3 w-3" strokeWidth={2.5} />
        </button>
        <button className="flex items-center justify-center transition-colors hover:text-red-500 focus:outline-none">
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
