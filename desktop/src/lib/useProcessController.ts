import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  checkInstanceHealth,
  killProcess,
  launchInstance,
  launchInstanceDirect,
  queryLaunchState,
  formatError,
  type HealthReport,
} from './tauri';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type LaunchPhase =
  | 'idle'
  | 'checking-health'
  | 'awaiting-decision'
  | 'launching'
  | 'running'
  | 'stopping'
  | 'delegated'
  | 'exited'
  | 'failed';

export interface ProcessState {
  phase: LaunchPhase;
  instanceId: string | null;
  pid: number | null;
  error: string | null;
  healthReport: HealthReport | null;
  /** The launch mode (direct vs delegated) captured at the start of the launch flow. */
  directLaunch: boolean;
  exitCode: number | null;
  outcome: 'success' | 'crash' | 'cancelled' | 'unknown' | 'abandoned' | null;
  snapshotId: string | null;
  exitedAt: string | null;
}

// ---------------------------------------------------------------------------
// Controller hook — intended to live at App level and survive page navigation.
// ---------------------------------------------------------------------------

export interface ProcessController {
  state: ProcessState;
  /** Bounded log buffer for the tracked instance. */
  logs: LogLine[];
  /** Start a health-gated launch. Shows the health dialog when warnings/blockers exist. */
  /** Returns true only when a launch command actually started. Health-deferred,
   * concurrent, and failed attempts return false. */
  startLaunch: (instanceId: string, directLaunch: boolean) => Promise<boolean>;
  /** Continue a launch after the user approved health warnings. Uses the mode captured in startLaunch. Returns null on success or an error string. */
  approveLaunch: () => Promise<string | null>;
  /** Cancel the launch flow (health dialog dismissal). */
  cancelLaunch: () => void;
  /** Kill the running process. */
  kill: () => Promise<void>;
  /** Clear a terminal error. */
  clearError: () => void;
}

const INITIAL_STATE: ProcessState = {
  phase: 'idle',
  instanceId: null,
  pid: null,
  error: null,
  healthReport: null,
  directLaunch: false,
  exitCode: null,
  outcome: null,
  snapshotId: null,
  exitedAt: null,
};

// Bounded log buffer per instance ID.
const MAX_LOG_LINES = 5000;

export interface LogLine {
  line: string;
  stream: 'stdout' | 'stderr';
  instance_id: string;
}

export function useProcessController(): ProcessController {
  const [state, setState] = useState<ProcessState>(INITIAL_STATE);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const stateRef = useRef(state);
  stateRef.current = state;

  // Hydrate from backend on mount — recover running state after reload.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const running = await queryLaunchState();
        if (!cancelled && running) {
          setState({
            phase: 'running',
            instanceId: running.instance_id,
            pid: running.pid,
            error: null,
            healthReport: null,
            directLaunch: true,
            exitCode: null,
            outcome: null,
            snapshotId: null,
            exitedAt: null,
          });
        }
      } catch {
        // Backend unavailable — stay with default idle state.
      }
    })();
    return () => { cancelled = true; };
  }, []);

  // Preserve the terminal outcome so users can see whether the last session
  // succeeded, crashed, or was cancelled after the process exits.
  useEffect(() => {
    const unlisten = listen<{
      instance_id: string;
      exit_code: number | null;
      outcome: 'success' | 'crash' | 'cancelled' | 'unknown' | 'abandoned';
      snapshot_id: string;
    }>(
      'game-exited',
      (event) => {
        const current = stateRef.current;
        if (
          current.instanceId === event.payload.instance_id &&
          (current.phase === 'running' || current.phase === 'stopping' || current.phase === 'delegated')
        ) {
          setState((previous) => ({
            ...previous,
            phase: 'exited',
            pid: null,
            error: null,
            healthReport: null,
            exitCode: event.payload.exit_code,
            outcome: event.payload.outcome,
            snapshotId: event.payload.snapshot_id,
            exitedAt: new Date().toISOString(),
          }));
        }
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Listen for game-log events and buffer them.
  useEffect(() => {
    const unlisten = listen<{ line: string; stream: string; instance_id: string }>(
      'game-log',
      (event) => {
        const current = stateRef.current;
        // Only buffer logs for the tracked instance.
        if (current.instanceId !== event.payload.instance_id) return;
        setLogs((prev) => {
          const next = [...prev, {
            line: event.payload.line,
            stream: event.payload.stream as 'stdout' | 'stderr',
            instance_id: event.payload.instance_id,
          }];
          if (next.length > MAX_LOG_LINES) {
            return next.slice(-MAX_LOG_LINES);
          }
          return next;
        });
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const startLaunch = useCallback(
    async (instanceId: string, directLaunch: boolean) => {
      // Reject if any non-terminal phase is active (concurrent-launch guard).
      const current = stateRef.current;
      const activePhases: LaunchPhase[] = ['checking-health', 'awaiting-decision', 'launching', 'running'];
      if (activePhases.includes(current.phase)) {
        setState((prev) => ({
          ...prev,
          error: 'A launch is already in progress. Wait for it to complete before launching another instance.',
        }));
        return false;
      }

      setState({
        phase: 'checking-health',
        instanceId,
        pid: null,
        error: null,
        healthReport: null,
        directLaunch,
        exitCode: null,
        outcome: null,
        snapshotId: null,
        exitedAt: null,
      });

      try {
        const report = await checkInstanceHealth(instanceId);
        const hasBlockers = report.blockers.length > 0;
        const hasWarnings = report.warnings.length > 0;

        if (hasBlockers || hasWarnings) {
          setState((prev) => ({
            ...prev,
            phase: 'awaiting-decision',
            healthReport: report,
          }));
          return false;
        }

        // All clear — launch immediately with the captured mode.
        const newState = await executeLaunch(instanceId, directLaunch);
        setState(newState);
        return true;
      } catch (e) {
        setState((prev) => ({
          ...prev,
          phase: 'failed',
          error: formatError(e),
        }));
        return false;
      }
    },
    [],
  );

  const approveLaunch = useCallback(
    async (): Promise<string | null> => {
      const current = stateRef.current;
      if (!current.instanceId) return 'No instance selected';

      setState((prev) => ({ ...prev, phase: 'launching', error: null, healthReport: prev.healthReport }));

      try {
        const newState = await executeLaunch(current.instanceId, current.directLaunch);
        setState(newState);
        return null;
      } catch (e) {
        const msg = formatError(e);
        // Stay in awaiting-decision so the HealthDialog remains open
        // with the error visible. The user can try again or cancel.
        setState((prev) => ({
          ...prev,
          phase: 'awaiting-decision',
          error: msg,
        }));
        return msg;
      }
    },
    [],
  );

  const cancelLaunch = useCallback(() => {
    setState(INITIAL_STATE);
  }, []);

  const kill = useCallback(async () => {
    const current = stateRef.current;
    // Delegated launches have no owned PID — nothing to kill.
    if (current.pid == null) return;
    setState((previous) => ({ ...previous, phase: 'stopping', error: null }));
    try {
      await killProcess(current.pid);
      // The backend retains ownership until its process waiter emits the
      // classified game-exited event.
    } catch (e) {
      setState((prev) => ({
        ...prev,
        phase: 'running',
        error: formatError(e),
      }));
    }
  }, []);

  const clearError = useCallback(() => {
    setState((prev) => ({ ...prev, error: null }));
  }, []);

  return {
    state,
    logs,
    startLaunch,
    approveLaunch,
    cancelLaunch,
    kill,
    clearError,
  };
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function launchedState(
  instanceId: string,
  directLaunch: boolean,
  pid: number | null,
): ProcessState {
  return {
    phase: directLaunch ? 'running' : 'delegated',
    instanceId,
    pid: directLaunch ? pid : null,
    error: null,
    healthReport: null,
    directLaunch,
    exitCode: null,
    outcome: null,
    snapshotId: null,
    exitedAt: null,
  };
}

async function executeLaunch(
  instanceId: string,
  directLaunch: boolean,
): Promise<ProcessState> {
  if (directLaunch) {
    const pid = await launchInstanceDirect(instanceId);
    return launchedState(instanceId, true, pid);
  } else {
    await launchInstance(instanceId);
    return launchedState(instanceId, false, null);
  }
}
