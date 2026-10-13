import { useEffect, useMemo, useState } from "react";
import {
  createUserDependency,
  deleteUserDependency,
  listDependencies,
} from "./api";
import type {
  CanonicalCapability,
  DependencyKind,
  DependencyRelationship,
} from "./model";

type DependenciesPageProps = {
  capabilities: CanonicalCapability[];
  initialRevision: string;
};

type DependencyErrorShape = {
  code?: string;
};

function dependencyKindLabel(kind: DependencyKind) {
  return kind === "requires" ? "Requires" : "Depends on";
}

function dependencyErrorCode(error: unknown) {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof (error as DependencyErrorShape).code === "string"
  ) {
    return (error as DependencyErrorShape).code ?? "";
  }

  return "";
}

export default function DependenciesPage({
  capabilities,
  initialRevision,
}: DependenciesPageProps) {
  const [relationships, setRelationships] = useState<DependencyRelationship[]>([]);
  const [revision, setRevision] = useState(initialRevision);
  const [loading, setLoading] = useState(true);
  const [mutating, setMutating] = useState(false);
  const [message, setMessage] = useState("");
  const [sourceCapabilityId, setSourceCapabilityId] = useState("");
  const [kind, setKind] = useState<DependencyKind>("requires");
  const [targetCapabilityId, setTargetCapabilityId] = useState("");

  const capabilityById = useMemo(
    () =>
      new Map(
        capabilities.map((capability) => [capability.id, capability] as const),
      ),
    [capabilities],
  );

  const sortedCapabilities = useMemo(
    () =>
      [...capabilities].sort(
        (left, right) =>
          left.canonicalName.localeCompare(right.canonicalName) ||
          left.id.localeCompare(right.id),
      ),
    [capabilities],
  );

  const sortedRelationships = useMemo(
    () =>
      [...relationships].sort((left, right) => {
        const leftSource =
          capabilityById.get(left.sourceCapabilityId)?.canonicalName ??
          left.sourceCapabilityId;
        const rightSource =
          capabilityById.get(right.sourceCapabilityId)?.canonicalName ??
          right.sourceCapabilityId;
        const leftTarget =
          capabilityById.get(left.targetCapabilityId)?.canonicalName ??
          left.targetCapabilityId;
        const rightTarget =
          capabilityById.get(right.targetCapabilityId)?.canonicalName ??
          right.targetCapabilityId;

        return (
          leftSource.localeCompare(rightSource) ||
          left.kind.localeCompare(right.kind) ||
          leftTarget.localeCompare(rightTarget) ||
          left.id.localeCompare(right.id)
        );
      }),
    [relationships, capabilityById],
  );

  useEffect(() => {
    let cancelled = false;

    void listDependencies()
      .then((snapshot) => {
        if (cancelled) return;
        setRelationships(snapshot.relationships);
        setRevision(snapshot.revision);
      })
      .catch(() => {
        if (cancelled) return;
        setMessage("Could not load local relationship data.");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  async function recoverFromConflict() {
    try {
      const snapshot = await listDependencies();
      setRelationships(snapshot.relationships);
      setRevision(snapshot.revision);
      setMessage("Relationship data changed. Review the latest list and retry.");
    } catch {
      setMessage("Relationship data changed, but the latest list could not be loaded.");
    }
  }

  async function handleMutationError(error: unknown) {
    const code = dependencyErrorCode(error);

    if (code === "conflict") {
      await recoverFromConflict();
      return;
    }

    if (code === "dependency_duplicate") {
      setMessage("That relationship already exists.");
      return;
    }

    if (code === "dependency_self_reference") {
      setMessage("A capability cannot have a relationship with itself.");
      return;
    }

    if (code === "dependency_invalid" || code === "invalid_request") {
      setMessage("Invalid relationship. Choose valid reviewed capabilities.");
      return;
    }

    if (code === "dependency_not_user_owned") {
      setMessage("Scanner relationships are read-only and are not user-owned.");
      return;
    }

    setMessage("Could not update local relationship data.");
  }

  async function addRelationship() {
    if (
      !sourceCapabilityId ||
      !targetCapabilityId ||
      sourceCapabilityId === targetCapabilityId ||
      mutating
    ) {
      return;
    }

    setMutating(true);
    setMessage("");

    try {
      const response = await createUserDependency(
        sourceCapabilityId,
        kind,
        targetCapabilityId,
        revision,
      );
      setRelationships(response.relationships);
      setRevision(response.revision);
    } catch (error) {
      await handleMutationError(error);
    } finally {
      setMutating(false);
    }
  }

  async function removeRelationship(relationshipId: string) {
    if (mutating) return;

    setMutating(true);
    setMessage("");

    try {
      const response = await deleteUserDependency(relationshipId, revision);
      setRelationships(response.relationships);
      setRevision(response.revision);
    } catch (error) {
      await handleMutationError(error);
    } finally {
      setMutating(false);
    }
  }

  const selfReference =
    sourceCapabilityId !== "" &&
    sourceCapabilityId === targetCapabilityId;

  return (
    <section className="panel dependencies-page">
      <div className="dependencies-intro">
        <div>
          <p className="section-kicker">Authoritative relationships</p>
          <h2>Dependency workspace</h2>
        </div>
        <p>
          Relationships are shown only when they are persisted. Installation
          names, paths and visual proximity never create dependency edges.
        </p>
      </div>

      {message && <div role="alert" className="dependency-alert">{message}</div>}

      <section className="dependency-list-section" aria-label="Dependency relationships">
        <div className="section-heading">
          <h3>Relationships</h3>
          <span>{loading ? "Loading..." : `${relationships.length} persisted`}</span>
        </div>

        {!loading && sortedRelationships.length === 0 ? (
          <p className="dependency-empty">No persisted relationships.</p>
        ) : (
          <div className="dependency-list">
            {sortedRelationships.map((relationship) => {
              const source =
                capabilityById.get(relationship.sourceCapabilityId)?.canonicalName ??
                relationship.sourceCapabilityId;
              const target =
                capabilityById.get(relationship.targetCapabilityId)?.canonicalName ??
                relationship.targetCapabilityId;

              return (
                <article
                  key={relationship.id}
                  className="dependency-row"
                  data-dependency-row
                  data-testid={`dependency-row-${relationship.id}`}
                >
                  <div className="dependency-endpoint">
                    <small>Source</small>
                    <strong>{source}</strong>
                  </div>

                  <div className="dependency-kind">
                    <small>Relationship</small>
                    <strong>{dependencyKindLabel(relationship.kind)}</strong>
                  </div>

                  <div className="dependency-endpoint">
                    <small>Target</small>
                    <strong>{target}</strong>
                  </div>

                  <div className="dependency-owner">
                    <small>Owner</small>
                    {relationship.owner === "scanner" ? (
                      <>
                        <strong>Scanner</strong>
                        <span>Read-only</span>
                        <span
                          title={relationship.provenance}
                          aria-label={`Provenance ${relationship.provenance}`}
                        >
                          Provenance: {relationship.provenance.replace(/^scanner:/i, "")}
                        </span>
                      </>
                    ) : (
                      <>
                        <strong>User</strong>
                        <button
                          type="button"
                          onClick={() => void removeRelationship(relationship.id)}
                          disabled={mutating}
                        >
                          Delete
                        </button>
                      </>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </section>

      <section className="dependency-add-section" aria-label="Add relationship">
        <div className="section-heading">
          <h3>Add relationship</h3>
          <span>Reviewed capabilities only</span>
        </div>

        <div className="dependency-form">
          <label>
            Source capability
            <select
              aria-label="Source capability"
              value={sourceCapabilityId}
              onChange={(event) => setSourceCapabilityId(event.target.value)}
            >
              <option value="">Select source</option>
              {sortedCapabilities.map((capability) => (
                <option
                  key={capability.id}
                  value={capability.id}
                  aria-label={capability.canonicalName}
                >
                  Source: {capability.canonicalName}
                </option>
              ))}
            </select>
          </label>

          <label>
            Relationship type
            <select
              aria-label="Relationship type"
              value={kind}
              onChange={(event) => setKind(event.target.value as DependencyKind)}
            >
              <option value="requires" aria-label="Requires">Type: Requires</option>
              <option value="depends_on" aria-label="Depends on">Type: Depends on</option>
            </select>
          </label>

          <label>
            Target capability
            <select
              aria-label="Target capability"
              value={targetCapabilityId}
              onChange={(event) => setTargetCapabilityId(event.target.value)}
            >
              <option value="">Select target</option>
              {sortedCapabilities.map((capability) => (
                <option
                  key={capability.id}
                  value={capability.id}
                  aria-label={capability.canonicalName}
                >
                  Target: {capability.canonicalName}
                </option>
              ))}
            </select>
          </label>

          <button
            type="button"
            onClick={() => void addRelationship()}
            disabled={
              loading ||
              mutating ||
              !sourceCapabilityId ||
              !targetCapabilityId ||
              selfReference
            }
          >
            Add relationship
          </button>
        </div>

        {selfReference && (
          <p className="dependency-form-note">
            Source and target must be different capabilities.
          </p>
        )}
      </section>

      <DependencyGraph
        relationships={sortedRelationships}
        capabilityById={capabilityById}
      />
    </section>
  );
}

function DependencyGraph({
  relationships,
  capabilityById,
}: {
  relationships: DependencyRelationship[];
  capabilityById: Map<string, CanonicalCapability>;
}) {
  const nodes = useMemo(() => {
    const ids = new Set<string>();

    for (const relationship of relationships) {
      ids.add(relationship.sourceCapabilityId);
      ids.add(relationship.targetCapabilityId);
    }

    return [...ids]
      .map((id) => ({
        id,
        name: capabilityById.get(id)?.canonicalName ?? id,
      }))
      .sort(
        (left, right) =>
          left.name.localeCompare(right.name) || left.id.localeCompare(right.id),
      );
  }, [relationships, capabilityById]);

  const positions = useMemo(() => {
    const result = new Map<string, { x: number; y: number }>();
    const columns = 3;

    nodes.forEach((node, index) => {
      result.set(node.id, {
        x: 120 + (index % columns) * 220,
        y: 90 + Math.floor(index / columns) * 150,
      });
    });

    return result;
  }, [nodes]);

  const rows = Math.max(1, Math.ceil(nodes.length / 3));
  const height = 80 + rows * 150;

  return (
    <section className="dependency-graph-section">
      <div className="section-heading">
        <h3>Capability graph</h3>
        <span>Read-only</span>
      </div>

      <svg
        className="dependency-graph"
        aria-label="Dependency graph"
        role="img"
        viewBox={`0 0 720 ${height}`}
      >
        {relationships.map((relationship) => {
          const source = positions.get(relationship.sourceCapabilityId);
          const target = positions.get(relationship.targetCapabilityId);

          if (!source || !target) return null;

          const sourceName =
            capabilityById.get(relationship.sourceCapabilityId)?.canonicalName ??
            relationship.sourceCapabilityId;
          const targetName =
            capabilityById.get(relationship.targetCapabilityId)?.canonicalName ??
            relationship.targetCapabilityId;

          const midX = (source.x + target.x) / 2;
          const midY = (source.y + target.y) / 2;

          return (
            <g
              key={relationship.id}
              data-dependency-edge={relationship.id}
              data-source-capability-id={relationship.sourceCapabilityId}
              data-target-capability-id={relationship.targetCapabilityId}
              data-dependency-kind={relationship.kind}
              aria-label={`${sourceName} ${dependencyKindLabel(relationship.kind)} ${targetName}`}
            >
              <line
                x1={source.x}
                y1={source.y}
                x2={target.x}
                y2={target.y}
                className="dependency-edge-line"
              />
              <text
                x={midX}
                y={midY - 8}
                textAnchor="middle"
                className="dependency-edge-label"
              >
                Edge: {dependencyKindLabel(relationship.kind)}
              </text>
            </g>
          );
        })}

        {nodes.map((node) => {
          const position = positions.get(node.id);
          if (!position) return null;

          return (
            <g
              key={node.id}
              data-capability-node={node.id}
              aria-label={node.name}
            >
              <rect
                x={position.x - 78}
                y={position.y - 28}
                width="156"
                height="56"
                rx="12"
                className="dependency-node"
              />
              <text
                x={position.x}
                y={position.y + 5}
                textAnchor="middle"
                className="dependency-node-label"
              >
                Node: {node.name}
              </text>
            </g>
          );
        })}
      </svg>
    </section>
  );
}
