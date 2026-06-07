import { Shield, Minus, Square, X } from 'lucide-react';
import type { MouseEvent } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

type WindowCommand = 'minimize' | 'toggleMaximize' | 'close' | 'startDragging';

async function runWindowCommand(command: WindowCommand) {
  if (!('__TAURI_INTERNALS__' in window)) {
    return;
  }

  const appWindow = getCurrentWindow();
  await appWindow[command]();
}

export function TitleBar() {
  const handleMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest('button')) {
      return;
    }

    if (event.buttons !== 1) {
      return;
    }

    event.preventDefault();
    void runWindowCommand('startDragging');
  };

  return (
    <div
      className="relative z-50 flex h-12 w-full select-none items-center justify-between px-4 text-slate-700"
      onMouseDown={handleMouseDown}
    >
      <div className="flex items-center gap-2">
        <Shield className="h-4 w-4 text-[#0066b8]" strokeWidth={2.5} />
        <span className="text-sm font-medium tracking-wide">WinFaceUnlock</span>
      </div>
      <div className="flex items-center gap-5 text-slate-500">
        <button
          onClick={() => void runWindowCommand('minimize')}
          className="flex items-center justify-center transition-colors hover:text-slate-900 focus:outline-none"
          aria-label="最小化"
        >
          <Minus className="h-4 w-4" />
        </button>
        <button
          onClick={() => void runWindowCommand('toggleMaximize')}
          className="flex items-center justify-center transition-colors hover:text-slate-900 focus:outline-none"
          aria-label="最大化"
        >
          <Square className="h-3 w-3" strokeWidth={2.5} />
        </button>
        <button
          onClick={() => void runWindowCommand('close')}
          className="flex items-center justify-center transition-colors hover:text-red-500 focus:outline-none"
          aria-label="关闭"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
