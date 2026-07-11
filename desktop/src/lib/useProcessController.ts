import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  checkInstanceHealth,
  killProcess,
  launchInstance,
  launchInstanceDirect,
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
}

// ---------------------------------------------------------------------------
// Controller hook — intended to live at App level and survive page navigation.
// ---------------------------------------------------------------------------

export interface ProcessController {
  state: ProcessState;
  /** Start a health-gated launch. Shows the health dialog when warnings/blockers exist. */
  startLaunch: (instanceId: string, directLaunch: boolean) => Promise<void>;
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
};

export function useProcessController(): ProcessController {
  const [state, setState] = useState<ProcessState>(INITIAL_STATE);
  const stateRef = useRef(state);
  stateRef.current = state;

  // Listen for game-exited events to clear running state.
  useEffect(() => {
    const unlisten = listen<{ instance_id: string; exit_code: number }>(
      'game-exited',
      (event) => {
        const current = stateRef.current;
        if (
          current.instanceId === event.payload.instance_id &&
          current.phase === 'running'
        ) {
          setState(INITIAL_STATE);
        }
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const startLaunch = useCallback(
    async (instanceId: string, directLaunch: boolean) => {
      setState({
        phase: 'checking-health',
        instanceId,
        pid: null,
        error: null,
        healthReport: null,
        directLaunch,
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
          return;
        }

        // All clear — launch immediately with the captured mode.
        const newState = await executeLaunch(instanceId, directLaunch);
        setState(newState);
      } catch (e) {
        setState((prev) => ({
          ...prev,
          phase: 'failed',
          error: formatError(e),
        }));
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
    try {
      await killProcess(current.pid);
      setState(INITIAL_STATE);
    } catch (e) {
      setState((prev) => ({
        ...prev,
        error: formatError(e),
      }));
    }
  }, []);

  const clearError = useCallback(() => {
    setState((prev) => ({ ...prev, error: null }));
  }, []);

  return {
    state,
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
    phase: directLaunch ? 'running' : 'exited',
    instanceId,
    pid: directLaunch ? pid : null,
    error: null,
    healthReport: null,
    directLaunch,
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
