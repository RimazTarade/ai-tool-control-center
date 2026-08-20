import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BootstrapState, Discovery, ScanEvent, ScanHandle, ScanMutationRequest, ScanRequest, ScanState } from "./model";

const synthetic: Discovery[] = [
  {
    id: "demo-1",
    suggested_name: "Example local runtime",
    suggested_type: "runtime",
    source_scanner: "demo.fixture",
    confidence: "high",
    evidence: [{ kind: "demo", summary: "Synthetic sample — not observed on this computer" }],
    observed_at: "2026-01-01T00:00:00Z",
    health_state: "unknown",
  },
  {
    id: "demo-2",
    suggested_name: "Example MCP registration",
    suggested_type: "mcp",
    source_scanner: "demo.fixture",
    confidence: "medium",
    evidence: [{ kind: "demo", summary: "Synthetic configuration evidence" }],
    observed_at: "2026-01-01T00:00:00Z",
    health_state: "unknown",
  },
];

export const isDesktop = () => "__TAURI_INTERNALS__" in window;

export async function bootstrap(): Promise<BootstrapState> {
  if (isDesktop()) return invoke<BootstrapState>("bootstrap_state");
  return { mode: "demo", pending: synthetic, inventory: [], scanRevision: "demo-revision" };
}

export async function reviewDiscovery(id: string, decision: "import" | "ignore" | "unknown"): Promise<void> {
  if (isDesktop()) await invoke("review_discovery", { id, decision });
}

export const pickScanRoots = () => invoke<string[]>("pick_scan_roots");

/// Attaches the `scan:event` listener before invoking `start_scan` so an
/// immediate `started` event emitted synchronously by the backend cannot be
/// missed by the frontend.
export async function startScan(
  request: ScanRequest,
  onEvent: (event: ScanEvent) => void,
): Promise<{ handle: ScanHandle; unlisten: UnlistenFn }> {
  const unlisten = await listen<ScanEvent>("scan:event", ({ payload }) => {
    onEvent(payload);
  });

  try {
    const handle = await invoke<ScanHandle>("start_scan", { request });
    return { handle, unlisten };
  } catch (error) {
    unlisten();
    throw error;
  }
}

export const pauseScan = (request: ScanMutationRequest) => invoke<ScanState>("pause_scan", { request });

export const resumeScan = (request: ScanMutationRequest) => invoke<ScanState>("resume_scan", { request });

export const cancelScan = (request: ScanMutationRequest) => invoke<ScanState>("cancel_scan", { request });
