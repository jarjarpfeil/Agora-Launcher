import { open as openUrl } from '@tauri-apps/plugin-shell';
import type { DeviceFlowResponse } from '../lib/tauri';

interface DeviceFlowPanelProps {
  device: DeviceFlowResponse;
  polling: boolean;
  onCancel: () => void;
  className?: string;
}

export function DeviceFlowPanel({ device, polling, onCancel, className = '' }: DeviceFlowPanelProps) {
  const copy = (value: string) => {
    navigator.clipboard.writeText(value).catch(() => {
      // The visible value remains available for manual copy.
    });
  };

  return (
    <div className={`rounded-lg border border-border bg-muted p-3 space-y-2 ${className}`}>
      <p className="text-xs">Opening your browser… If it did not open, use the button below.</p>
      <p className="text-sm font-semibold text-primary break-all">{device.verification_uri}</p>
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span>Code:</span>
        <span className="font-mono font-bold tracking-widest">{device.user_code}</span>
        <button
          type="button"
          onClick={() => copy(device.user_code)}
          className="rounded border border-border px-2 py-0.5 text-[10px] font-medium hover:bg-accent"
        >
          Copy Code
        </button>
      </div>
      {device.expires_in > 0 && (
        <p className="text-xs text-muted-foreground">
          Code is valid for {Math.floor(device.expires_in / 60)}:{String(device.expires_in % 60).padStart(2, '0')}.
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => openUrl(device.verification_uri).catch(() => {})}
          className="rounded-lg border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent"
        >
          Open in browser
        </button>
        <button
          type="button"
          onClick={() => copy(device.verification_uri)}
          className="rounded-lg border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent"
        >
          Copy URL
        </button>
        {polling && (
          <button
            type="button"
            onClick={onCancel}
            className="rounded-lg border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent"
          >
            Cancel
          </button>
        )}
      </div>
      {polling && <p className="text-xs text-muted-foreground">Waiting for authorization…</p>}
    </div>
  );
}
