import { useEffect, useMemo, useRef, useState } from "react";
import { bootstrap, cancelQuickScan, review, startQuickScan } from "./api";
import type { BootstrapState, Discovery, PageName } from "./model";

const pages: PageName[] = ["Overview", "Inventory", "Review Queue", "Health", "Dependencies", "Activity", "Adapter Packs", "Backups", "Settings"];

export default function App() {
  const [page, setPage] = useState<PageName>("Overview");
  const [state, setState] = useState<BootstrapState>({ mode: "demo", pending: [], inventory: [] });
  const [query, setQuery] = useState("");
  const [scan, setScan] = useState<{ active: boolean; paused: boolean; progress: number; visited: number; id?: string; notice?: string }>({ active: false, paused: false, progress: 0, visited: 0 });
  const stopListening = useRef<null | (() => void)>(null);

  useEffect(() => { void bootstrap().then(setState); }, []);
  useEffect(() => () => stopListening.current?.(), []);
  useEffect(() => {
    if (state.mode !== "demo" || !scan.active || scan.paused) return;
    const timer = window.setInterval(() => setScan((current) => {
      const progress = Math.min(100, current.progress + 4);
      return { ...current, progress, active: progress < 100 };
    }), 120);
    return () => window.clearInterval(timer);
  }, [scan.active, scan.paused, state.mode]);

  const visible = useMemo(() => state.pending.filter((item) => item.suggested_name.toLowerCase().includes(query.toLowerCase())), [query, state.pending]);

  async function decide(item: Discovery, decision: "import" | "ignore" | "unknown") {
    await review(item.id, decision);
    setState((current) => ({
      ...current,
      pending: current.pending.filter((candidate) => candidate.id !== item.id),
      inventory: decision === "import" ? [...current.inventory, item] : current.inventory,
    }));
  }

  async function runScan() {
    if (scan.active) return;
    setScan({ active: true, paused: false, progress: 0, visited: 0 });
    if (state.mode === "demo") return;
    let endedBeforeStartReturned = false;
    try {
      const running = await startQuickScan((event) => {
        if (event.kind === "progress") setScan((current) => ({ ...current, visited: event.visited ?? current.visited }));
        if (event.kind === "scanner_failed") setScan((current) => ({ ...current, notice: event.message ?? "Part of the scan could not complete." }));
        if (["completed", "cancelled", "failed"].includes(event.kind)) {
          endedBeforeStartReturned = true;
          stopListening.current?.();
          stopListening.current = null;
          setScan((current) => ({ ...current, active: false, progress: event.kind === "completed" ? 100 : current.progress, notice: event.kind === "failed" ? (event.message ?? "The scan could not complete.") : current.notice }));
          void bootstrap().then(setState);
        }
      });
      if (running) {
        if (endedBeforeStartReturned) {
          running.unlisten();
        } else {
          stopListening.current = running.unlisten;
          setScan((current) => ({ ...current, id: running.id }));
        }
      }
    } catch {
      setScan((current) => ({ ...current, active: false, notice: "The quick scan could not start." }));
    }
  }

  async function cancelScan() {
    let notice: string | undefined;
    try {
      if (scan.id) await cancelQuickScan(scan.id);
    } catch {
      notice = "Cancellation could not be confirmed.";
    } finally {
      stopListening.current?.();
      stopListening.current = null;
      setScan({ active: false, paused: false, progress: 0, visited: 0, notice });
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
            <button className="scan" disabled={scan.active} onClick={() => void runScan()}>Run quick scan</button>
          </div>
        </header>
        {state.mode === "demo" && <div className="demo" role="status">Demo mode · all records are synthetic and do not describe this computer.</div>}
        {scan.active && <section className="scanbar" aria-label="Scan progress"><div><strong>{scan.paused ? "Scan paused" : "Scanning local sources"}</strong><span>{state.mode === "demo" ? `${scan.progress}%` : `${scan.visited} locations checked`}</span></div>{state.mode === "demo" ? <progress value={scan.progress} max="100" /> : <progress />}<footer>{state.mode === "demo" && <button onClick={() => setScan((current) => ({ ...current, paused: !current.paused }))}>{scan.paused ? "Resume" : "Pause"}</button>}<button disabled={state.mode === "desktop" && !scan.id} onClick={() => void cancelScan()}>Cancel</button></footer></section>}
        {scan.notice && <div className="scan-warning" role="alert">{scan.notice}</div>}
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
