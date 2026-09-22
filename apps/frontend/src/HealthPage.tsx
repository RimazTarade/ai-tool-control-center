import type { CanonicalCapability, HealthState, InstallationInstance } from "./model";
import type { UseHealthValue } from "./useHealth";

type HealthPageProps = {
  health: UseHealthValue;
  installations: InstallationInstance[];
  capabilities: CanonicalCapability[];
};

export function healthStateLabel(state: HealthState) {
  switch (state) {
    case "green":
      return "Healthy";
    case "yellow":
      return "Warning";
    case "red":
      return "Failed required check";
    case "blue":
      return "Setup required";
    case "grey":
      return "Not checked";
  }
}

export default function HealthPage({
  health,
  installations,
  capabilities,
}: HealthPageProps) {
  const activeCheckAll = Object.values(health.activeRuns).find(
    (run) =>
      run.scope === "all" &&
      (run.state === "running" || run.state === "cancelling"),
  );

  const capabilityById = new Map(
    capabilities.map((capability) => [capability.id, capability]),
  );

  const rows = health.workspace
    ? health.workspace.installations.map((snapshot) => ({
        installation: snapshot.installation,
        capabilityName: snapshot.capability.canonicalName,
        health: snapshot.effectiveHealth,
        lastCheckedAt: snapshot.lastCheckedAt,
        warningCount: snapshot.warningCount,
        failureCount: snapshot.failureCount,
      }))
    : installations.map((installation) => ({
        installation,
        capabilityName:
          capabilityById.get(installation.capabilityId)?.canonicalName ??
          installation.displayName,
        health: installation.aggregateHealth,
        lastCheckedAt: null,
        warningCount: 0,
        failureCount: 0,
      }));

  return (
    <section className="panel health-page">
      <div className="panel-title">
        <div>
          <p className="eyebrow">Reviewed installations</p>
          <h2>Health</h2>
          <p>Checks run only when you request them.</p>
        </div>
        <button
          type="button"
          disabled={health.loading || Boolean(activeCheckAll)}
          aria-busy={health.loading || Boolean(activeCheckAll)}
          onClick={() => void health.runAll()}
        >
          Check All
        </button>
      </div>

      {activeCheckAll && (
        <div className="health-run-progress" aria-label="Health check progress">
          <span>Requested {activeCheckAll.requestedInstallationCount}</span>
          <span>Scheduled {activeCheckAll.scheduledInstallationCount}</span>
          <span>Skipped {activeCheckAll.skippedActiveInstallationCount}</span>
          <button
            type="button"
            disabled={activeCheckAll.state === "cancelling"}
            aria-busy={activeCheckAll.state === "cancelling"}
            onClick={() => void health.cancelRun(activeCheckAll.runId)}
          >
            Cancel
          </button>
        </div>
      )}

      {rows.length === 0 ? (
        <p className="empty">No reviewed installations yet.</p>
      ) : (
        rows.map(({
          installation,
          capabilityName,
          health: state,
          lastCheckedAt,
          warningCount,
          failureCount,
        }) => (
          <article className="row" key={installation.id}>
            <div className="toolicon">
              {installation.displayName.slice(0, 2).toUpperCase()}
            </div>
            <button
              type="button"
              className="health-installation-select"
              onClick={() => void health.selectInstallation(installation.id)}
            >
              <strong>{installation.displayName}</strong>
              <span>{capabilityName}</span>
            </button>
            <em>{installation.host}</em>
            <div className="health-state">
              {activeCheckAll && <span>Checking</span>}
              <b>{healthStateLabel(state)}</b>
              <span>Last checked: {lastCheckedAt ?? "Not checked"}</span>
              <span>Warnings: {warningCount}</span>
              <span>Failures: {failureCount}</span>
            </div>
            <button
              type="button"
              disabled={health.loading}
              aria-busy={health.loading}
              onClick={() => void health.runInstallation(installation.id)}
            >
              Run checks
            </button>
          </article>
        ))
      )}

      {health.selected && (
        <section className="health-detail" aria-label="Installation health detail">
          <p className="eyebrow">Installation detail</p>
          <h3>{health.selected.snapshot.installation.displayName}</h3>
          <p>{health.selected.snapshot.capability.canonicalName}</p>
          <p>
            Current state: {healthStateLabel(health.selected.snapshot.effectiveHealth)}
          </p>
          <p>
            Last checked: {health.selected.snapshot.lastCheckedAt ?? "Not checked"}
          </p>

          {health.selected.snapshot.checks.length === 0 ? (
            <p className="empty">
              No supported local health checks are configured for this installation.
              Health remains Not checked until supported evidence is available.
            </p>
          ) : (
            <div className="health-checks" aria-label="Configured health checks">
              {health.selected.snapshot.checks.map((check) => (
                <article className="health-check" key={check.definition.id}>
                  <div>
                    <strong>{check.definition.checkType}</strong>
                    <span>{check.definition.required ? "Required" : "Optional"}</span>
                  </div>

                  {check.latestResult ? (
                    <>
                      <p>
                        State: {healthStateLabel(check.latestResult.state)}
                      </p>
                      <p>Result code: {check.latestResult.resultCode}</p>
                      <p>Observed: {check.latestResult.observedAt}</p>
                      <p>Duration: {check.latestResult.durationMs} ms</p>
                      {check.stale && <p>Stale</p>}
                      <p>{check.latestResult.redactedEvidence}</p>
                    </>
                  ) : (
                    <p>Not checked</p>
                  )}
                </article>
              ))}
            </div>
          )}

          {health.selected.history.length > 0 && (
            <div className="health-history" aria-label="Health history">
              <h4>Recent history</h4>
              {health.selected.history.map((result) => (
                <article key={result.id}>
                  <strong>{result.resultCode}</strong>
                  <span>{healthStateLabel(result.state)}</span>
                  <span>{result.observedAt}</span>
                  <span>{result.durationMs} ms</span>
                  <p>{result.redactedEvidence}</p>
                </article>
              ))}
            </div>
          )}
        </section>
      )}
    </section>
  );
}