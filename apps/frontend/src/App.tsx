import { useEffect, useMemo, useRef, useState } from "react";
import { bootstrap, cancelScan, pauseScan, pickScanRoots, resumeScan, reviewDiscovery, startScan } from "./api";
import type { BootstrapState, Discovery, PageName, ScanEvent, ScanLifecycleState, ScanMode, ScanRequest } from "./model";

const pages: PageName[] = ["Overview", "Inventory", "Review Queue", "Health", "Dependencies", "Activity", "Adapter Packs", "Backups", "Settings"];

type ScanDraft = {
  open: boolean;
  mode: ScanMode;
  roots: string[];
  followReparsePoints: boolean;
  networkConsent: boolean;
};

type ScanWarning = { scannerId: string; code: string; message: string };

type ActiveScan = {
  scanId: string;
  scope: ScanMode;
  state: ScanLifecycleState;
  revision: string;
  cancelling: boolean;
  scannerId?: string;
  completedUnits: number;
  totalUnits: number | null;
  currentLocation: string | null;
  warnings: ScanWarning[];
};

type TerminalNotice = {
  kind: "completed" | "cancelled" | "failed";
  message?: string;
  counts?: { visited: number; discovered: number; failureCount: number };
};

const defaultScanDraft: ScanDraft = {
  open: false,
  mode: "quick",
  roots: [],
  followReparsePoints: false,
  networkConsent: false,
};

export default function App() {
  const [page, setPage] = useState<PageName>("Overview");
  const [state, setState] = useState<BootstrapState>({ mode: "demo", pending: [], inventory: [], scanRevision: "" });
  const [workspaceRevision, setWorkspaceRevision] = useState("");
  const [query, setQuery] = useState("");
  const [scanDraft, setScanDraft] = useState<ScanDraft>(defaultScanDraft);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | undefined>();
  const [networkConfirmPending, setNetworkConfirmPending] = useState(false);
  const [activeScan, setActiveScan] = useState<ActiveScan | null>(null);
  const [terminalNotice, setTerminalNotice] = useState<TerminalNotice | null>(null);

  const stopListening = useRef<null | (() => void)>(null);
  const demoTimer = useRef<number | null>(null);
  const demoStep = useRef(0);

  useEffect(() => {
    void bootstrap().then((result) => {
      setState(result);
      setWorkspaceRevision(result.scanRevision);
    });
  }, []);

  useEffect(
    () => () => {
      stopListening.current?.();
      if (demoTimer.current) window.clearTimeout(demoTimer.current);
    },
    [],
  );

  const visible = useMemo(() => state.pending.filter((item) => item.suggested_name.toLowerCase().includes(query.toLowerCase())), [query, state.pending]);

  async function decide(item: Discovery, decision: "import" | "ignore" | "unknown") {
    await reviewDiscovery(item.id, decision);
    setState((current) => ({
      ...current,
      pending: current.pending.filter((candidate) => candidate.id !== item.id),
      inventory: decision === "import" ? [...current.inventory, item] : current.inventory,
    }));
  }

  function openDialog() {
    setScanDraft({ ...defaultScanDraft, open: true });
    setStartError(undefined);
    setNetworkConfirmPending(false);
  }

  function closeDialog() {
    setScanDraft({ ...defaultScanDraft, open: false });
    setStartError(undefined);
    setNetworkConfirmPending(false);
  }

  async function selectFolders() {
    const roots = await pickScanRoots();
    setScanDraft((current) => ({ ...current, roots }));
  }

  function buildRequest(networkConsent: boolean): ScanRequest {
    return {
      mode: scanDraft.mode,
      roots: scanDraft.mode === "deep" ? scanDraft.roots : [],
      followReparsePoints: scanDraft.mode === "deep" && scanDraft.followReparsePoints,
      networkConsent: scanDraft.mode === "deep" && networkConsent,
      revision: workspaceRevision,
    };
  }

  function handleScanEvent(event: ScanEvent) {
    switch (event.kind) {
      case "progress":
        setActiveScan((current) =>
          current
            ? {
                ...current,
                scannerId: event.scanner_id ?? current.scannerId,
                completedUnits: event.completed_units,
                totalUnits: event.total_units ?? null,
                currentLocation: event.current_location ?? current.currentLocation,
              }
            : current,
        );
        break;
      case "scanner_failed":
        setActiveScan((current) =>
          current
            ? { ...current, warnings: [...current.warnings, { scannerId: event.scanner_id, code: event.code, message: event.message }] }
            : current,
        );
        break;
      case "paused":
        setActiveScan((current) => (current ? { ...current, state: "paused" } : current));
        break;
      case "resumed":
        setActiveScan((current) => (current ? { ...current, state: "running" } : current));
        break;
      case "completed":
        void handleTerminal({ kind: "completed", counts: { visited: event.visited, discovered: event.discovered, failureCount: event.failure_count } });
        break;
      case "cancelled":
        void handleTerminal({ kind: "cancelled", counts: { visited: event.visited, discovered: event.discovered, failureCount: event.failure_count } });
        break;
      case "failed":
        void handleTerminal({ kind: "failed", message: event.message });
        break;
      default:
        break;
    }
  }

  async function handleTerminal(notice: TerminalNotice) {
    setTerminalNotice(notice);
    setActiveScan(null);
    stopListening.current?.();
    stopListening.current = null;
    if (demoTimer.current) {
      window.clearTimeout(demoTimer.current);
      demoTimer.current = null;
    }
    const result = await bootstrap();
    setWorkspaceRevision(result.scanRevision);
    setState(result);
  }

  async function attemptStart(request: ScanRequest) {
    setStarting(true);
    setStartError(undefined);
    try {
      if (state.mode === "demo") {
        startDemoScan(request);
      } else {
        const { handle, unlisten } = await startScan(request, handleScanEvent);
        stopListening.current = unlisten;
        setActiveScan({
          scanId: handle.scanId,
          scope: handle.scope,
          state: handle.state,
          revision: handle.revision,
          cancelling: false,
          completedUnits: 0,
          totalUnits: null,
          currentLocation: null,
          warnings: [],
        });
        closeDialog();
      }
    } catch (error) {
      const code = (error as { code?: string } | null)?.code;
      if (code === "network_consent_required" && request.mode === "deep") {
        setNetworkConfirmPending(true);
      } else {
        setStartError("The scan could not start.");
      }
    } finally {
      setStarting(false);
    }
  }

  function runScan() {
    void attemptStart(buildRequest(scanDraft.networkConsent));
  }

  function confirmNetworkConsent() {
    setScanDraft((current) => ({ ...current, networkConsent: true }));
    setNetworkConfirmPending(false);
    void attemptStart(buildRequest(true));
  }

  function cancelNetworkConsent() {
    setNetworkConfirmPending(false);
  }

  function startDemoScan(request: ScanRequest) {
    demoStep.current = 0;
    setActiveScan({
      scanId: `demo-scan-${Date.now()}`,
      scope: request.mode,
      state: "running",
      revision: "demo-revision",
      cancelling: false,
      completedUnits: 0,
      totalUnits: 5,
      currentLocation: null,
      warnings: [],
    });
    closeDialog();
    runDemoTick();
  }

  function runDemoTick() {
    demoTimer.current = window.setTimeout(() => {
      demoStep.current += 1;
      const step = demoStep.current;
      if (step === 3) {
        handleScanEvent({ kind: "scanner_failed", scanner_id: "demo.scanner", code: "demo_recoverable", message: "A demo location could not be read." });
        runDemoTick();
        return;
      }
      if (step >= 6) {
        handleScanEvent({ kind: "completed", visited: 5, discovered: 2, failure_count: 1, duration_ms: step * 220 });
        return;
      }
      handleScanEvent({ kind: "progress", scanner_id: "demo.scanner", completed_units: step, total_units: 5, current_location: `Example location ${step}` });
      runDemoTick();
    }, 220);
  }

  function pauseDemo() {
    if (demoTimer.current) {
      window.clearTimeout(demoTimer.current);
      demoTimer.current = null;
    }
    handleScanEvent({ kind: "paused" });
  }

  function resumeDemo() {
    handleScanEvent({ kind: "resumed" });
    runDemoTick();
  }

  function cancelDemo() {
    if (demoTimer.current) {
      window.clearTimeout(demoTimer.current);
      demoTimer.current = null;
    }
    handleScanEvent({ kind: "cancelled", visited: demoStep.current, discovered: 0, failure_count: 0, duration_ms: demoStep.current * 220 });
  }

  async function onPause() {
    if (!activeScan) return;
    if (state.mode === "demo") {
      pauseDemo();
      return;
    }
    try {
      const result = await pauseScan({ scanId: activeScan.scanId, revision: activeScan.revision });
      setActiveScan((current) => (current ? { ...current, revision: result.revision, state: result.state } : current));
    } catch {
      // the next scan:event will reconcile state; nothing else to do here.
    }
  }

  async function onResume() {
    if (!activeScan) return;
    if (state.mode === "demo") {
      resumeDemo();
      return;
    }
    try {
      const result = await resumeScan({ scanId: activeScan.scanId, revision: activeScan.revision });
      setActiveScan((current) => (current ? { ...current, revision: result.revision, state: result.state } : current));
    } catch {
      // the next scan:event will reconcile state; nothing else to do here.
    }
  }

  async function onCancel() {
    if (!activeScan) return;
    if (state.mode === "demo") {
      cancelDemo();
      return;
    }
    setActiveScan((current) => (current ? { ...current, cancelling: true } : current));
    try {
      const result = await cancelScan({ scanId: activeScan.scanId, revision: activeScan.revision });
      setActiveScan((current) => (current ? { ...current, revision: result.revision } : current));
    } catch {
      // the next scan:event will reconcile state; nothing else to do here.
    }
  }

  return (
    <div className="shell">
      <aside>
        <div className="brand"><span className="mark">AI</span><div><strong>Tool Control</strong><small>Local command center</small></div></div>
        <nav aria-label="Main navigation">
          {pages.map((name) => <button key={name} aria-label={name} className={page === name ? "active" : ""} onClick={() => setPage(name)}><span aria-hidden="true">{name.slice(0, 2)}</span>{name}{name === "Review Queue" && state.pending.length > 0 && <b>{state.pending.length}</b>}</button>)}
        </nav>
        <div className="privacy"><i /> Local only · zero telemetry</div>
      </aside>
      <main>
        <header>
          <div><p className="eyebrow">Workspace / {page}</p><h1>{page}</h1></div>
          <div className="header-actions">
            <label className="search"><span>⌕</span><input aria-label="Search discoveries" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search local tools" /></label>
            <button className="scan" disabled={!!activeScan} onClick={openDialog}>Run scan</button>
          </div>
        </header>
        {state.mode === "demo" && <div className="demo" role="status">Demo mode · all records are synthetic and do not describe this computer.</div>}
        {scanDraft.open && (
          <div className="modal-overlay">
            <div className="modal" role="dialog" aria-label="Run scan">
              <h2>Run scan</h2>
              <fieldset className="scan-mode">
                <legend>Scan type</legend>
                <label>
                  <input type="radio" name="scanMode" checked={scanDraft.mode === "quick"} onChange={() => setScanDraft((current) => ({ ...current, mode: "quick" }))} />
                  Quick
                </label>
                <label>
                  <input type="radio" name="scanMode" checked={scanDraft.mode === "deep"} onChange={() => setScanDraft((current) => ({ ...current, mode: "deep" }))} />
                  Deep
                </label>
              </fieldset>
              {scanDraft.mode === "deep" && (
                <div className="deep-options">
                  <button type="button" onClick={() => void selectFolders()}>Select folders</button>
                  <div className="roots-list">
                    {scanDraft.roots.length === 0 ? (
                      <p className="empty-roots">No folders selected.</p>
                    ) : (
                      <ul>{scanDraft.roots.map((root) => <li key={root}>{root}</li>)}</ul>
                    )}
                  </div>
                  <label className="checkbox-row">
                    <input
                      type="checkbox"
                      checked={scanDraft.followReparsePoints}
                      onChange={(event) => setScanDraft((current) => ({ ...current, followReparsePoints: event.target.checked }))}
                    />
                    Follow symbolic links and junctions
                  </label>
                </div>
              )}
              {networkConfirmPending && (
                <div className="network-confirm" role="alertdialog" aria-label="Network location confirmation">
                  <p>One or more selected roots are on a network location. Allow this Deep Scan to read those network roots once?</p>
                  <div>
                    <button type="button" onClick={confirmNetworkConsent}>Allow</button>
                    <button type="button" className="quiet" onClick={cancelNetworkConsent}>Cancel</button>
                  </div>
                </div>
              )}
              {startError && <p role="alert">{startError}</p>}
              <div className="modal-actions">
                <button type="button" className="quiet" onClick={closeDialog}>Close</button>
                <button type="button" disabled={starting || (scanDraft.mode === "deep" && scanDraft.roots.length === 0)} onClick={runScan}>Run</button>
              </div>
            </div>
          </div>
        )}
        {activeScan && (
          <section className="scanbar" aria-label="Scan progress">
            <div>
              <strong>
                {activeScan.scope === "deep" ? "Deep scan" : "Quick scan"} ·{" "}
                {activeScan.cancelling ? "Cancelling" : activeScan.state === "paused" ? "Paused" : "Running"}
              </strong>
              <span>
                {activeScan.scannerId ? `${activeScan.scannerId} · ` : ""}
                {activeScan.completedUnits}
                {activeScan.totalUnits != null ? ` / ${activeScan.totalUnits}` : ""}
                {activeScan.currentLocation ? ` · ${activeScan.currentLocation}` : ""}
              </span>
            </div>
            <footer>
              {activeScan.state === "running" && !activeScan.cancelling && <button onClick={() => void onPause()}>Pause</button>}
              {activeScan.state === "paused" && !activeScan.cancelling && <button onClick={() => void onResume()}>Resume</button>}
              <button disabled={activeScan.cancelling} onClick={() => void onCancel()}>Cancel</button>
            </footer>
            {activeScan.warnings.map((warning, index) => (
              <p key={`${warning.scannerId}-${warning.code}-${index}`} role="alert" className="scan-warning-inline">
                {warning.scannerId} · {warning.code}: {warning.message}
              </p>
            ))}
          </section>
        )}
        {terminalNotice && (
          <div className="scan-notice" role="status">
            {terminalNotice.kind === "failed"
              ? terminalNotice.message ?? "The scan could not complete."
              : `Scan ${terminalNotice.kind}: ${terminalNotice.counts?.discovered ?? 0} discoveries, ${terminalNotice.counts?.visited ?? 0} visited, ${terminalNotice.counts?.failureCount ?? 0} failures.`}
          </div>
        )}
        {page === "Overview" ? <Overview state={state} onReview={() => setPage("Review Queue")} /> : page === "Review Queue" ? <ReviewQueue items={visible} decide={decide} /> : page === "Inventory" ? <Inventory items={state.inventory} /> : <EmptyPage name={page} />}
      </main>
    </div>
  );
}

function Overview({ state, onReview }: { state: BootstrapState; onReview: () => void }) {
  return <><section className="metrics"><Metric label="Reviewed tools" value={state.inventory.length} tone="mint" /><Metric label="Pending review" value={state.pending.length} tone="amber" /><Metric label="Health failures" value={0} tone="rose" /><Metric label="Unknown state" value={state.pending.length} tone="blue" /></section><section className="panel hero"><div><p className="eyebrow">Next best action</p><h2>Review discoveries before they enter inventory</h2><p>Every scanner observation stays pending until you explicitly import, ignore, or retain it as unknown.</p></div><button onClick={onReview}>Review {state.pending.length} items →</button></section><section className="panel"><div className="panel-title"><div><p className="eyebrow">Recent evidence</p><h2>Pending discoveries</h2></div><span>{state.pending.length} observations</span></div>{state.pending.slice(0, 4).map((item) => <EvidenceRow key={item.id} item={item} />)}</section></>;
}

function Metric({ label, value, tone }: { label: string; value: number; tone: string }) { return <article className={`metric ${tone}`}><span>{label}</span><strong>{value.toString().padStart(2, "0")}</strong><small>installation instances</small></article>; }
function EvidenceRow({ item }: { item: Discovery }) { return <article className="row"><div className="toolicon">{item.suggested_name.slice(0, 2).toUpperCase()}</div><div><strong>{item.suggested_name}</strong><span>{item.evidence[0]?.summary}</span></div><em>{item.confidence} confidence</em><b>Unknown</b></article>; }
function ReviewQueue({ items, decide }: { items: Discovery[]; decide: (item: Discovery, decision: "import" | "ignore" | "unknown") => void }) { return <section className="panel"><div className="panel-title"><div><p className="eyebrow">Mandatory gate</p><h2>{items.length} discoveries need a decision</h2></div></div>{items.length === 0 ? <p className="empty">No matching pending discoveries.</p> : items.map((item) => <article className="review" key={item.id}><EvidenceRow item={item} /><p>Detected by {item.source_scanner}. Health remains unknown until a reviewed check runs.</p><div><button onClick={() => void decide(item, "import")}>Import</button><button onClick={() => void decide(item, "unknown")}>Keep unknown</button><button className="quiet" onClick={() => void decide(item, "ignore")}>Ignore once</button></div></article>)}</section>; }
function Inventory({ items }: { items: Discovery[] }) { return <section className="panel"><div className="panel-title"><div><p className="eyebrow">Reviewed only</p><h2>Inventory</h2></div></div>{items.length ? items.map((item) => <EvidenceRow key={item.id} item={item} />) : <p className="empty">Nothing imported yet. Review a discovery first.</p>}</section>; }
function EmptyPage({ name }: { name: PageName }) { return <section className="panel empty-page"><span>{name.slice(0, 2)}</span><h2>{name}</h2><p>This workspace will populate from reviewed local evidence. No synthetic status is shown.</p><button disabled title="Available after reviewed evidence exists">No reviewed data yet</button></section>; }
