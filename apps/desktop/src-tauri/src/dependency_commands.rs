#[cfg(test)]
use control_center_core::Store;
use control_center_core::{DependencyKind, DependencyRelationship, StoreError};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DependencyCommandError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteUserDependencyRequest {
    pub(crate) relationship_id: String,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DependencyWorkspaceSnapshot {
    pub(crate) relationships: Vec<DependencyRelationship>,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateUserDependencyRequest {
    pub(crate) source_capability_id: String,
    pub(crate) kind: DependencyKind,
    pub(crate) target_capability_id: String,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DependencyMutationResponse {
    pub(crate) relationships: Vec<DependencyRelationship>,
    pub(crate) revision: String,
}

#[cfg(test)]
fn create_user_dependency_from_request(
    registry: &DependencyRegistry,
    store: &mut Store,
    request: CreateUserDependencyRequest,
) -> Result<DependencyMutationResponse, DependencyCommandError> {
    let source_capability_id = Uuid::parse_str(&request.source_capability_id)
        .map_err(|_| DependencyCommandError::invalid_request())?;
    let target_capability_id = Uuid::parse_str(&request.target_capability_id)
        .map_err(|_| DependencyCommandError::invalid_request())?;

    create_user_dependency_in_store(
        registry,
        store,
        source_capability_id,
        request.kind,
        target_capability_id,
        &request.revision,
    )
}

#[cfg(test)]
fn create_user_dependency_in_store(
    registry: &DependencyRegistry,
    store: &mut Store,
    source_capability_id: Uuid,
    kind: DependencyKind,
    target_capability_id: Uuid,
    revision: &str,
) -> Result<DependencyMutationResponse, DependencyCommandError> {
    let permit = registry.admit(revision)?;

    if let Err(error) =
        store.create_user_dependency(source_capability_id, kind, target_capability_id)
    {
        let _ = registry.complete_failure(permit);
        return Err(map_dependency_store_error(error));
    }

    let revision = registry.complete_success(permit)?;
    let relationships = store.dependencies().map_err(map_dependency_store_error)?;

    Ok(DependencyMutationResponse {
        relationships,
        revision,
    })
}

#[cfg(test)]
fn delete_user_dependency_in_store(
    registry: &DependencyRegistry,
    store: &mut Store,
    relationship_id: Uuid,
    revision: &str,
) -> Result<DependencyMutationResponse, DependencyCommandError> {
    let permit = registry.admit(revision)?;

    if let Err(error) = store.delete_user_dependency(relationship_id) {
        let _ = registry.complete_failure(permit);
        return Err(map_dependency_store_error(error));
    }

    let revision = registry.complete_success(permit)?;
    let relationships = store.dependencies().map_err(map_dependency_store_error)?;

    Ok(DependencyMutationResponse {
        relationships,
        revision,
    })
}

fn map_dependency_store_error(error: StoreError) -> DependencyCommandError {
    match error {
        StoreError::DependencyDuplicate => DependencyCommandError {
            code: "dependency_duplicate",
            message: "That dependency already exists".into(),
        },
        StoreError::DependencySelfReference => DependencyCommandError {
            code: "dependency_self_reference",
            message: "A capability cannot depend on itself".into(),
        },
        StoreError::DependencyInvalid => DependencyCommandError {
            code: "dependency_invalid",
            message: "One or more dependency endpoints are invalid".into(),
        },
        StoreError::NotFound => DependencyCommandError {
            code: "not_found",
            message: "Dependency relationship was not found".into(),
        },
        StoreError::DependencyNotUserOwned => DependencyCommandError {
            code: "dependency_not_user_owned",
            message: "Scanner-owned dependencies cannot be deleted by the user".into(),
        },
        StoreError::Database(_)
        | StoreError::InvalidData(_)
        | StoreError::StorageIntegrity(_)
        | StoreError::NonTerminalFinishState(_) => DependencyCommandError {
            code: "storage_integrity",
            message: "Local storage is unavailable".into(),
        },
    }
}

#[tauri::command]
pub(crate) fn list_dependencies(
    state: tauri::State<'_, crate::scan_commands::AppState>,
) -> Result<DependencyWorkspaceSnapshot, DependencyCommandError> {
    list_dependencies_from_state(&state)
}

#[tauri::command]
pub(crate) fn create_user_dependency(
    request: CreateUserDependencyRequest,
    state: tauri::State<'_, crate::scan_commands::AppState>,
) -> Result<DependencyMutationResponse, DependencyCommandError> {
    create_user_dependency_from_state(&state, request)
}

#[tauri::command]
pub(crate) fn delete_user_dependency(
    request: DeleteUserDependencyRequest,
    state: tauri::State<'_, crate::scan_commands::AppState>,
) -> Result<DependencyMutationResponse, DependencyCommandError> {
    delete_user_dependency_from_state(&state, request)
}

fn create_user_dependency_from_state(
    state: &crate::scan_commands::AppState,
    request: CreateUserDependencyRequest,
) -> Result<DependencyMutationResponse, DependencyCommandError> {
    let source_capability_id = Uuid::parse_str(&request.source_capability_id)
        .map_err(|_| DependencyCommandError::invalid_request())?;
    let target_capability_id = Uuid::parse_str(&request.target_capability_id)
        .map_err(|_| DependencyCommandError::invalid_request())?;

    let permit = state.dependencies.admit(&request.revision)?;
    let mutation = {
        let mut store = match state.store.lock() {
            Ok(store) => store,
            Err(_) => {
                let _ = state.dependencies.complete_failure(permit);
                return Err(DependencyCommandError {
                    code: "storage_integrity",
                    message: "Local storage is unavailable".into(),
                });
            }
        };
        store.create_user_dependency(source_capability_id, request.kind, target_capability_id)
    };

    if let Err(error) = mutation {
        state.dependencies.complete_failure(permit)?;
        return Err(map_dependency_store_error(error));
    }

    let revision = state.dependencies.complete_success(permit)?;
    let relationships = {
        let store = state.store.lock().map_err(|_| DependencyCommandError {
            code: "storage_integrity",
            message: "Local storage is unavailable".into(),
        })?;
        store.dependencies().map_err(map_dependency_store_error)?
    };

    Ok(DependencyMutationResponse {
        relationships,
        revision,
    })
}

fn delete_user_dependency_from_state(
    state: &crate::scan_commands::AppState,
    request: DeleteUserDependencyRequest,
) -> Result<DependencyMutationResponse, DependencyCommandError> {
    let relationship_id = Uuid::parse_str(&request.relationship_id)
        .map_err(|_| DependencyCommandError::invalid_request())?;

    let permit = state.dependencies.admit(&request.revision)?;
    let mutation = {
        let mut store = match state.store.lock() {
            Ok(store) => store,
            Err(_) => {
                let _ = state.dependencies.complete_failure(permit);
                return Err(DependencyCommandError {
                    code: "storage_integrity",
                    message: "Local storage is unavailable".into(),
                });
            }
        };
        store.delete_user_dependency(relationship_id)
    };

    if let Err(error) = mutation {
        state.dependencies.complete_failure(permit)?;
        return Err(map_dependency_store_error(error));
    }

    let revision = state.dependencies.complete_success(permit)?;
    let relationships = {
        let store = state.store.lock().map_err(|_| DependencyCommandError {
            code: "storage_integrity",
            message: "Local storage is unavailable".into(),
        })?;
        store.dependencies().map_err(map_dependency_store_error)?
    };

    Ok(DependencyMutationResponse {
        relationships,
        revision,
    })
}

fn list_dependencies_from_state(
    state: &crate::scan_commands::AppState,
) -> Result<DependencyWorkspaceSnapshot, DependencyCommandError> {
    let revision = state.dependencies.workspace_revision();
    let relationships = {
        let store = state.store.lock().map_err(|_| DependencyCommandError {
            code: "storage_integrity",
            message: "Local storage is unavailable".into(),
        })?;
        store.dependencies().map_err(map_dependency_store_error)?
    };

    Ok(DependencyWorkspaceSnapshot {
        relationships,
        revision,
    })
}

#[cfg(test)]
fn build_dependency_workspace_snapshot(
    registry: &DependencyRegistry,
    store: &Store,
) -> Result<DependencyWorkspaceSnapshot, DependencyCommandError> {
    let revision = registry.workspace_revision();
    let relationships = store.dependencies().map_err(map_dependency_store_error)?;
    Ok(DependencyWorkspaceSnapshot {
        relationships,
        revision,
    })
}

impl DependencyCommandError {
    fn conflict() -> Self {
        Self {
            code: "conflict",
            message: "Dependency workspace changed; refresh and try again".into(),
        }
    }

    fn invalid_request() -> Self {
        Self {
            code: "invalid_request",
            message: "Dependency request is invalid".into(),
        }
    }
}

#[derive(Debug)]
struct DependencyRegistryState {
    workspace_revision: String,
    in_flight: Option<Uuid>,
}

#[derive(Debug)]
pub(crate) struct DependencyRegistry {
    state: Mutex<DependencyRegistryState>,
}

impl DependencyRegistry {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(DependencyRegistryState {
                workspace_revision: Uuid::new_v4().to_string(),
                in_flight: None,
            }),
        }
    }

    pub(crate) fn workspace_revision(&self) -> String {
        self.state.lock().unwrap().workspace_revision.clone()
    }

    pub(crate) fn admit(&self, revision: &str) -> Result<Uuid, DependencyCommandError> {
        let mut state = self.state.lock().unwrap();
        if state.workspace_revision != revision || state.in_flight.is_some() {
            return Err(DependencyCommandError::conflict());
        }

        let permit = Uuid::new_v4();
        state.in_flight = Some(permit);
        Ok(permit)
    }

    pub(crate) fn complete_failure(&self, permit: Uuid) -> Result<(), DependencyCommandError> {
        let mut state = self.state.lock().unwrap();
        if state.in_flight != Some(permit) {
            return Err(DependencyCommandError::conflict());
        }

        state.in_flight = None;
        Ok(())
    }

    pub(crate) fn complete_success(&self, permit: Uuid) -> Result<String, DependencyCommandError> {
        let mut state = self.state.lock().unwrap();
        if state.in_flight != Some(permit) {
            return Err(DependencyCommandError::conflict());
        }

        state.workspace_revision = Uuid::new_v4().to_string();
        state.in_flight = None;
        Ok(state.workspace_revision.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan_commands::AppState;
    use control_center_core::{
        CanonicalCapability, DependencyKind, DependencyProposal, Discovery, ReviewDecision, Store,
    };
    use uuid::Uuid;

    fn import_capability(
        store: &mut Store,
        name: &str,
        fingerprint: &str,
        stable_identity: &str,
    ) -> CanonicalCapability {
        let mut discovery =
            Discovery::unknown(name, "dependency-command-fixture", fingerprint.into());
        discovery.canonical_identity = Some(stable_identity.to_string());
        store.enqueue(&discovery).unwrap();
        store.review(discovery.id, ReviewDecision::Import).unwrap();
        store
            .capabilities()
            .unwrap()
            .into_iter()
            .find(|item| item.stable_identity.as_deref() == Some(stable_identity))
            .unwrap()
    }

    #[test]
    fn dependency_command_list_returns_authoritative_relationships_and_revision() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-list-source",
            "dependency:list:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-list-target",
            "dependency:list:target",
        );
        let relationship = store
            .create_user_dependency(source.id, DependencyKind::Requires, target.id)
            .unwrap();
        let registry = DependencyRegistry::new();
        let expected_revision = registry.workspace_revision();

        let snapshot = build_dependency_workspace_snapshot(&registry, &store).unwrap();

        assert_eq!(snapshot.revision, expected_revision);
        assert_eq!(snapshot.relationships, vec![relationship]);
    }

    #[test]
    fn dependency_command_create_rotates_revision_once() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-create-source",
            "dependency:create:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-create-target",
            "dependency:create:target",
        );
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let response = create_user_dependency_in_store(
            &registry,
            &mut store,
            source.id,
            DependencyKind::Requires,
            target.id,
            &revision,
        )
        .unwrap();

        assert_ne!(response.revision, revision);
        assert_eq!(registry.workspace_revision(), response.revision);
        assert_eq!(response.relationships, store.dependencies().unwrap());
        assert_eq!(response.relationships.len(), 1);
    }

    #[test]
    fn dependency_command_delete_rotates_revision_once() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-delete-source",
            "dependency:delete:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-delete-target",
            "dependency:delete:target",
        );
        let relationship = store
            .create_user_dependency(source.id, DependencyKind::Requires, target.id)
            .unwrap();
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let response =
            delete_user_dependency_in_store(&registry, &mut store, relationship.id, &revision)
                .unwrap();

        assert_ne!(response.revision, revision);
        assert_eq!(registry.workspace_revision(), response.revision);
        assert!(response.relationships.is_empty());
        assert!(store.dependencies().unwrap().is_empty());
    }

    #[test]
    fn dependency_command_duplicate_maps_dependency_duplicate() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-duplicate-source",
            "dependency:duplicate:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-duplicate-target",
            "dependency:duplicate:target",
        );
        store
            .create_user_dependency(source.id, DependencyKind::Requires, target.id)
            .unwrap();
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let error = create_user_dependency_in_store(
            &registry,
            &mut store,
            source.id,
            DependencyKind::Requires,
            target.id,
            &revision,
        )
        .unwrap_err();

        assert_eq!(error.code, "dependency_duplicate");
        assert_eq!(registry.workspace_revision(), revision);
        assert_eq!(store.dependencies().unwrap().len(), 1);
    }

    #[test]
    fn dependency_command_self_reference_maps_dependency_self_reference() {
        let mut store = Store::in_memory().unwrap();
        let capability =
            import_capability(&mut store, "Self", "dependency-self", "dependency:self");
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let error = create_user_dependency_in_store(
            &registry,
            &mut store,
            capability.id,
            DependencyKind::Requires,
            capability.id,
            &revision,
        )
        .unwrap_err();

        assert_eq!(error.code, "dependency_self_reference");
        assert_eq!(registry.workspace_revision(), revision);
        assert!(store.dependencies().unwrap().is_empty());
    }

    #[test]
    fn dependency_command_invalid_endpoint_maps_dependency_invalid() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-invalid-source",
            "dependency:invalid:source",
        );
        let missing_target = Uuid::new_v4();
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let error = create_user_dependency_in_store(
            &registry,
            &mut store,
            source.id,
            DependencyKind::Requires,
            missing_target,
            &revision,
        )
        .unwrap_err();

        assert_eq!(error.code, "dependency_invalid");
        assert_eq!(registry.workspace_revision(), revision);
        assert!(store.dependencies().unwrap().is_empty());
    }

    #[test]
    fn dependency_command_not_found_maps_not_found() {
        let mut store = Store::in_memory().unwrap();
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let error =
            delete_user_dependency_in_store(&registry, &mut store, Uuid::new_v4(), &revision)
                .unwrap_err();

        assert_eq!(error.code, "not_found");
        assert_eq!(registry.workspace_revision(), revision);
        assert!(store.dependencies().unwrap().is_empty());
    }

    #[test]
    fn dependency_command_scanner_owned_delete_maps_dependency_not_user_owned() {
        let mut store = Store::in_memory().unwrap();
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-scanner-owned-target",
            "dependency:scanner-owned:target",
        );

        let mut source_discovery = Discovery::unknown(
            "Source",
            "dependency-command-fixture",
            "dependency-scanner-owned-source".into(),
        );
        source_discovery.canonical_identity = Some("dependency:scanner-owned:source".into());
        source_discovery
            .dependency_proposals
            .push(DependencyProposal {
                kind: DependencyKind::Requires,
                target_stable_identity: "dependency:scanner-owned:target".into(),
                evidence_summary: "scanner observed dependency".into(),
            });
        store.enqueue(&source_discovery).unwrap();
        store
            .review(source_discovery.id, ReviewDecision::Import)
            .unwrap();

        let relationship = store
            .dependencies()
            .unwrap()
            .into_iter()
            .find(|item| item.target_capability_id == target.id)
            .unwrap();
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let error =
            delete_user_dependency_in_store(&registry, &mut store, relationship.id, &revision)
                .unwrap_err();

        assert_eq!(error.code, "dependency_not_user_owned");
        assert_eq!(registry.workspace_revision(), revision);
        assert_eq!(store.dependencies().unwrap(), vec![relationship]);
    }

    #[test]
    fn dependency_command_invalid_uuid_returns_invalid_request_before_store_or_revision() {
        let mut store = Store::in_memory().unwrap();
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-invalid-uuid-target",
            "dependency:invalid-uuid:target",
        );
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let request = CreateUserDependencyRequest {
            source_capability_id: "not-a-uuid".into(),
            kind: DependencyKind::Requires,
            target_capability_id: target.id.to_string(),
            revision: revision.clone(),
        };

        let error =
            create_user_dependency_from_request(&registry, &mut store, request).unwrap_err();

        assert_eq!(error.code, "invalid_request");
        assert_eq!(registry.workspace_revision(), revision);
        assert!(store.dependencies().unwrap().is_empty());
    }

    #[test]
    fn dependency_command_stale_create_revision_conflicts_without_store_mutation() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-stale-create-source",
            "dependency:stale-create:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-stale-create-target",
            "dependency:stale-create:target",
        );
        let registry = DependencyRegistry::new();
        let current_revision = registry.workspace_revision();
        let request = CreateUserDependencyRequest {
            source_capability_id: source.id.to_string(),
            kind: DependencyKind::Requires,
            target_capability_id: target.id.to_string(),
            revision: "stale-revision".into(),
        };

        let error =
            create_user_dependency_from_request(&registry, &mut store, request).unwrap_err();

        assert_eq!(error.code, "conflict");
        assert_eq!(registry.workspace_revision(), current_revision);
        assert!(store.dependencies().unwrap().is_empty());
    }

    #[test]
    fn dependency_command_stale_delete_revision_conflicts_without_store_mutation() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-stale-delete-source",
            "dependency:stale-delete:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-stale-delete-target",
            "dependency:stale-delete:target",
        );
        let relationship = store
            .create_user_dependency(source.id, DependencyKind::Requires, target.id)
            .unwrap();
        let registry = DependencyRegistry::new();
        let current_revision = registry.workspace_revision();

        let error = delete_user_dependency_in_store(
            &registry,
            &mut store,
            relationship.id,
            "stale-revision",
        )
        .unwrap_err();

        assert_eq!(error.code, "conflict");
        assert_eq!(registry.workspace_revision(), current_revision);
        assert_eq!(store.dependencies().unwrap(), vec![relationship]);
    }

    #[test]
    fn dependency_command_storage_details_are_redacted() {
        let secret_detail = r#"C:\sensitive\control-center.sqlite: evidence_token=super-secret"#;
        let error = map_dependency_store_error(StoreError::StorageIntegrity(secret_detail.into()));

        assert_eq!(error.code, "storage_integrity");
        assert_eq!(error.message, "Local storage is unavailable");
        assert!(!error.message.contains("rimaz"));
        assert!(!error.message.contains("control-center.sqlite"));
        assert!(!error.message.contains("super-secret"));
    }

    #[test]
    fn dependency_command_state_delete_rotates_revision_and_returns_authoritative_relationships() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-state-delete-source",
            "dependency:state-delete:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-state-delete-target",
            "dependency:state-delete:target",
        );
        let relationship = store
            .create_user_dependency(source.id, DependencyKind::Requires, target.id)
            .unwrap();
        let state = AppState::new(store);
        let revision = state.dependencies.workspace_revision();
        let request = DeleteUserDependencyRequest {
            relationship_id: relationship.id.to_string(),
            revision: revision.clone(),
        };

        let response = delete_user_dependency_from_state(&state, request).unwrap();

        assert_ne!(response.revision, revision);
        assert_eq!(state.dependencies.workspace_revision(), response.revision);
        assert!(response.relationships.is_empty());
    }

    #[test]
    fn dependency_command_state_create_rotates_revision_and_returns_authoritative_relationships() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-state-create-source",
            "dependency:state-create:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-state-create-target",
            "dependency:state-create:target",
        );
        let state = AppState::new(store);
        let revision = state.dependencies.workspace_revision();
        let request = CreateUserDependencyRequest {
            source_capability_id: source.id.to_string(),
            kind: DependencyKind::Requires,
            target_capability_id: target.id.to_string(),
            revision: revision.clone(),
        };

        let response = create_user_dependency_from_state(&state, request).unwrap();

        assert_ne!(response.revision, revision);
        assert_eq!(state.dependencies.workspace_revision(), response.revision);
        assert_eq!(response.relationships.len(), 1);
        assert_eq!(response.relationships[0].source_capability_id, source.id);
        assert_eq!(response.relationships[0].target_capability_id, target.id);
        assert_eq!(response.relationships[0].kind, DependencyKind::Requires);
    }

    #[test]
    fn dependency_command_list_and_mutation_lock_order_does_not_deadlock() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-lock-order-source",
            "dependency:lock-order:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-lock-order-target",
            "dependency:lock-order:target",
        );
        let state = std::sync::Arc::new(AppState::new(store));
        let revision = state.dependencies.workspace_revision();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let (tx, rx) = std::sync::mpsc::channel();

        let list_state = std::sync::Arc::clone(&state);
        let list_barrier = std::sync::Arc::clone(&barrier);
        let list_tx = tx.clone();
        let list_thread = std::thread::spawn(move || {
            list_barrier.wait();
            let ok = list_dependencies_from_state(&list_state).is_ok();
            list_tx.send(("list", ok)).unwrap();
        });

        let mutation_state = std::sync::Arc::clone(&state);
        let mutation_barrier = std::sync::Arc::clone(&barrier);
        let mutation_tx = tx.clone();
        let mutation_thread = std::thread::spawn(move || {
            let request = CreateUserDependencyRequest {
                source_capability_id: source.id.to_string(),
                kind: DependencyKind::Requires,
                target_capability_id: target.id.to_string(),
                revision,
            };
            mutation_barrier.wait();
            let ok = create_user_dependency_from_state(&mutation_state, request).is_ok();
            mutation_tx.send(("mutation", ok)).unwrap();
        });

        barrier.wait();
        drop(tx);

        let first = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("first dependency operation should complete without deadlock");
        let second = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("second dependency operation should complete without deadlock");

        assert!(first.1, "{} operation failed", first.0);
        assert!(second.1, "{} operation failed", second.0);
        list_thread.join().unwrap();
        mutation_thread.join().unwrap();
    }

    #[test]
    fn dependency_command_state_list_returns_authoritative_relationships_and_revision() {
        let mut store = Store::in_memory().unwrap();
        let source = import_capability(
            &mut store,
            "Source",
            "dependency-state-list-source",
            "dependency:state-list:source",
        );
        let target = import_capability(
            &mut store,
            "Target",
            "dependency-state-list-target",
            "dependency:state-list:target",
        );
        let relationship = store
            .create_user_dependency(source.id, DependencyKind::Requires, target.id)
            .unwrap();
        let state = AppState::new(store);
        let expected_revision = state.dependencies.workspace_revision();

        let snapshot = list_dependencies_from_state(&state).unwrap();

        assert_eq!(snapshot.revision, expected_revision);
        assert_eq!(snapshot.relationships, vec![relationship]);
    }

    #[test]
    fn dependency_command_revisions_are_independent_from_scan_and_health() {
        let state = AppState::new(Store::in_memory().unwrap());
        let scan_revision = state.scans.workspace_revision();
        let health_revision = state.health.workspace_revision();
        let dependency_revision = state.dependencies.workspace_revision();

        let permit = state.dependencies.admit(&dependency_revision).unwrap();
        let next_dependency_revision = state.dependencies.complete_success(permit).unwrap();

        assert_ne!(next_dependency_revision, dependency_revision);
        assert_eq!(state.scans.workspace_revision(), scan_revision);
        assert_eq!(state.health.workspace_revision(), health_revision);
        assert_eq!(
            state.dependencies.workspace_revision(),
            next_dependency_revision
        );
    }

    #[test]
    fn dependency_command_second_mutation_is_rejected_while_permit_active() {
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();
        let first = registry.admit(&revision).unwrap();

        let error = registry.admit(&revision).unwrap_err();

        assert_eq!(error.code, "conflict");
        assert_eq!(registry.workspace_revision(), revision);
        registry.complete_failure(first).unwrap();
    }

    #[test]
    fn dependency_command_failed_store_mutation_clears_permit_without_rotating_revision() {
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();
        let first = registry.admit(&revision).unwrap();

        registry.complete_failure(first).unwrap();

        assert_eq!(registry.workspace_revision(), revision);
        let second = registry.admit(&revision).unwrap();
        registry.complete_failure(second).unwrap();
    }

    #[test]
    fn dependency_command_successful_mutation_completes_matching_permit_only() {
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();
        let permit = registry.admit(&revision).unwrap();

        let error = registry.complete_success(Uuid::new_v4()).unwrap_err();

        assert_eq!(error.code, "conflict");
        assert_eq!(registry.workspace_revision(), revision);
        let next_revision = registry.complete_success(permit).unwrap();
        assert_ne!(next_revision, revision);
        assert_eq!(registry.workspace_revision(), next_revision);
    }

    #[test]
    fn dependency_command_stale_revision_conflicts_without_mutation() {
        let registry = DependencyRegistry::new();
        let revision = registry.workspace_revision();

        let error = registry.admit("stale-revision").unwrap_err();

        assert_eq!(error.code, "conflict");
        assert_eq!(registry.workspace_revision(), revision);
    }
}
