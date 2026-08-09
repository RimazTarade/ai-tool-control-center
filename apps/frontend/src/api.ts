import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BootstrapState, Discovery } from "./model";

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

const isDesktop = () => "__TAURI_INTERNALS__" in window;

export async function bootstrap(): Promise<BootstrapState> {
  if (isDesktop()) return invoke<BootstrapState>("bootstrap_state");
  return { mode: "demo", pending: synthetic, inventory: [] };
}

export async function review(id: string, decision: "import" | "ignore" | "unknown"): Promise<void> {
  if (isDesktop()) await invoke("review_discovery", { id, decision });
}

export type ScanEvent = { kind: string; visited?: number; discovered?: number; message?: string };

export async function startQuickScan(onEvent: (event: ScanEvent) => void): Promise<{ id: string; unlisten: UnlistenFn } | null> {
  if (!isDesktop()) return null;
  const unlisten = await listen<ScanEvent>("scan:event", (event) => onEvent(event.payload));
  try {
    const id = await invoke<string>("start_quick_scan");
    return { id, unlisten };
  } catch (error) {
    unlisten();
    throw error;
  }
}

export async function cancelQuickScan(id: string): Promise<void> {
  if (isDesktop()) await invoke("cancel_scan", { id });
}
