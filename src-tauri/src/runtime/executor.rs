use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use base64::Engine;
use chrono::{Duration, Utc};
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::app_state::{AppState, QueueGate};
use crate::bridge::{
    CommandError, CommandResult, GenerateImagesRequest, GeneratedAsset, GenerationCommandResult,
    ResponsePartDto, UsageDto,
};
use crate::domain;
use crate::providers::error::{ProviderError, ProviderErrorKind};
use crate::providers::types as provider;
use crate::providers::{ProviderAdapter, create_adapter};
use crate::storage::{
    ProjectStore, RemoteFileRecord, RemoteTaskRecord, StorageError, atomic_write_new,
};

use super::{
    PreparedGenerationRequest, build_batch_submission, prepare_generation_request,
    sanitize_provider_json,
};

pub async fn execute_generation(
    state: &AppState,
    request: GenerateImagesRequest,
    event_handler: Option<provider::EventHandler>,
) -> CommandResult<GenerationCommandResult> {
    if request.provider.id != request.draft.provider_id {
        return Err(CommandError::validation(
            "draft provider does not match the selected provider profile",
        ));
    }
    if !request.provider.enabled {
        return Err(CommandError::validation(
            "the selected provider is disabled",
        ));
    }
    let project = state
        .open_or_create_project(
            &request.project_id,
            &request.project_id,
            &request.storage_path,
        )
        .await?;
    let prepared = prepare_generation_request(&project, &request).await?;
    let run_id = prepared.provider_request.request_id.clone();
    let api_key = if request.provider.api_key.trim().is_empty() {
        state.secret(request.provider.credential_key()).await?
    } else {
        request.provider.api_key.trim().to_owned()
    };
    let config = state.resolved_provider_config(&request.provider).await?;
    let provider_kind = config.kind;
    let adapter: Arc<dyn ProviderAdapter> = Arc::from(create_adapter(
        config,
        request.provider.credentials(api_key),
    )?);
    let capability = prepared.capability_snapshot.clone();
    let selected_contexts = request.context_ids.iter().collect::<HashSet<_>>();
    let context_snapshot = if request.context_snapshot.is_empty() {
        project
            .list_prompt_contexts()
            .await?
            .into_iter()
            .filter(|context| selected_contexts.contains(&context.id))
            .collect()
    } else {
        request
            .context_snapshot
            .iter()
            .filter(|context| selected_contexts.contains(&context.id))
            .cloned()
            .collect()
    };
    let preset_snapshot = request
        .preset_snapshot
        .clone()
        .filter(|preset| request.preset_id.as_deref() == Some(preset.id.as_str()))
        .or(match request.preset_id.as_deref() {
            Some(preset_id) => project
                .list_presets()
                .await?
                .into_iter()
                .find(|preset| preset.id == preset_id),
            None => None,
        });
    let now = Utc::now();
    let mut run = domain::RunRecord {
        id: run_id.clone(),
        group_id: None,
        request: prepared.domain_request.clone(),
        status: domain::RunStatus::Queued,
        raw_prompt: request.draft.prompt.clone(),
        final_prompt: prepared.provider_request.prompt.clone(),
        context_snapshot,
        preset_snapshot,
        capability_snapshot: capability,
        capability_registry_version: "2026-07-12".to_owned(),
        model_version: None,
        provider_request_id: None,
        redacted_request: Some(sanitize_provider_json(&serde_json::to_value(
            &prepared.provider_request,
        )?)),
        redacted_response: None,
        started_at: None,
        finished_at: None,
        created_at: now,
        updated_at: now,
    };
    project.upsert_run(&run).await?;
    persist_inputs(&project, &prepared, &run_id).await?;

    let token = state.register_cancellation(&run_id).await;
    let result = if prepared.provider_request.execution == provider::ExecutionMode::ProviderBatch {
        execute_batch(
            state,
            &project,
            Arc::clone(&adapter),
            &request,
            &prepared,
            &mut run,
            token,
        )
        .await
    } else {
        execute_realtime(
            state,
            &project,
            adapter,
            &request,
            &prepared,
            &mut run,
            token,
            provider_kind,
            event_handler,
        )
        .await
    };
    state.finish_cancellation(&run_id).await;
    result
}

pub async fn poll_remote_tasks(
    state: &AppState,
    project_id: &str,
) -> CommandResult<Vec<GenerationCommandResult>> {
    let project = state.project(project_id).await?;
    let tasks = project.due_remote_tasks(Utc::now()).await?;
    let mut results = Vec::new();
    for mut task in tasks {
        let outcome: CommandResult<GenerationCommandResult> = async {
            let profile = state
                .global_store()
                .provider(&task.provider_profile_id)
                .await?
                .ok_or_else(|| {
                    CommandError::not_found(format!(
                        "provider profile {} is missing",
                        task.provider_profile_id
                    ))
                })?;
            let adapter = adapter_from_domain_profile(state, &profile).await?;
            let mut run = project.run(&task.run_id).await?.ok_or_else(|| {
                CommandError::not_found(format!("run {} is missing", task.run_id))
            })?;
            let mut job = if let Some(job_id) = &task.job_id {
                project.job(job_id).await?
            } else {
                project.list_jobs(&task.run_id).await?.into_iter().next()
            }
            .ok_or_else(|| {
                CommandError::not_found(format!("job for run {} is missing", task.run_id))
            })?;
            let kind = match task.task_type.as_str() {
                "batch" => provider::RemoteJobKind::Batch,
                _ => provider::RemoteJobKind::Background,
            };
            let remote = provider::RemoteJob {
                id: task.remote_id.clone(),
                kind,
                status: parse_remote_status(&task.status),
                provider: adapter.kind(),
                model: Some(run.request.model_id.clone()),
                raw: Value::Object(task.metadata.clone().into_iter().collect()),
            };
            let limit = state.provider_limit(&task.provider_profile_id).await;
            let token = CancellationToken::new();
            let polled = execute_cancellable(&token, limit, adapter.poll_job(&remote)).await?;
            task.status = remote_job_status(polled.job.status).to_owned();
            task.next_poll_at = matches!(
                polled.job.status,
                provider::RemoteJobStatus::Queued | provider::RemoteJobStatus::Running
            )
            .then(|| Utc::now() + Duration::seconds(5));
            task.metadata = sanitize_provider_json(&polled.job.raw)
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect();
            task.updated_at = Utc::now();
            project.upsert_remote_task(&task).await?;

            let mut result = GenerationCommandResult {
                run_id: run.id.clone(),
                request_id: polled.job.id.clone(),
                response_parts: vec![remote_job_part(&polled.job)],
                ..GenerationCommandResult::default()
            };
            let mut sequence = project.list_outputs(&run.id).await?.len() as u32;
            let mut download_failures = 0_usize;
            let mut failure_messages = Vec::new();
            let mut has_downloaded_content = false;
            for response in polled.outputs {
                record_continuation_id(&mut run, &response.raw);
                let processed = process_response(
                    &project,
                    Arc::clone(&adapter),
                    &task.provider_profile_id,
                    &run,
                    &job,
                    response,
                    &mut sequence,
                )
                .await?;
                download_failures += processed.failures.len();
                failure_messages
                    .extend(processed.failures.iter().map(|error| error.message.clone()));
                has_downloaded_content |= response_has_content(&processed.result);
                merge_result(&mut result, processed.result);
            }
            match polled.job.status {
                provider::RemoteJobStatus::Queued | provider::RemoteJobStatus::Running => {
                    job.status = domain::JobStatus::WaitingRemote;
                    run.status = domain::RunStatus::Running;
                }
                provider::RemoteJobStatus::Succeeded => {
                    job.status = if download_failures > 0 && !has_downloaded_content {
                        domain::JobStatus::Failed
                    } else {
                        domain::JobStatus::Succeeded
                    };
                    run.status = if download_failures == 0 {
                        domain::RunStatus::Succeeded
                    } else if has_downloaded_content {
                        domain::RunStatus::PartiallySucceeded
                    } else {
                        domain::RunStatus::Failed
                    };
                    run.finished_at = Some(Utc::now());
                }
                provider::RemoteJobStatus::Cancelled => {
                    job.status = domain::JobStatus::Cancelled;
                    run.status = domain::RunStatus::Cancelled;
                    run.finished_at = Some(Utc::now());
                }
                provider::RemoteJobStatus::Failed
                | provider::RemoteJobStatus::Expired
                | provider::RemoteJobStatus::Unknown => {
                    job.status = domain::JobStatus::Failed;
                    run.status = domain::RunStatus::Failed;
                    run.finished_at = Some(Utc::now());
                }
            }
            job.updated_at = Utc::now();
            run.provider_request_id = Some(polled.job.id.clone());
            run.redacted_response = Some(sanitize_provider_json(&polled.job.raw));
            run.updated_at = Utc::now();
            project.upsert_job(&job).await?;
            project.upsert_run(&run).await?;
            if !failure_messages.is_empty() {
                result.failure_reason = Some(failure_messages.join("; "));
            }
            finalize_result(&mut result, &run.id);
            Ok(result)
        }
        .await;
        match outcome {
            Ok(result) => results.push(result),
            Err(error) => {
                tracing::warn!(
                    remote_task_id = %task.id,
                    run_id = %task.run_id,
                    code = %error.code,
                    "remote task polling failed; scheduling retry"
                );
                record_remote_poll_failure(&project, &mut task, &error).await?;
            }
        }
    }
    Ok(results)
}

async fn record_remote_poll_failure(
    project: &ProjectStore,
    task: &mut RemoteTaskRecord,
    error: &CommandError,
) -> CommandResult<()> {
    let failures = task
        .metadata
        .get("pollFailures")
        .and_then(Value::as_u64)
        .unwrap_or_default()
        .saturating_add(1);
    task.metadata
        .insert("pollFailures".to_owned(), Value::from(failures));
    task.metadata.insert(
        "lastPollError".to_owned(),
        json!({ "code": error.code, "message": error.message }),
    );
    task.next_poll_at = Some(Utc::now() + remote_poll_backoff(failures));
    task.updated_at = Utc::now();
    project.upsert_remote_task(task).await?;
    if project.run(&task.run_id).await?.is_some() {
        project
            .save_error(&domain::ErrorRecord {
                id: Uuid::new_v4().to_string(),
                run_id: task.run_id.clone(),
                job_id: task.job_id.clone(),
                error: domain::ProviderError {
                    code: error.code.clone(),
                    message: error.message.clone(),
                    http_status: None,
                    retryable: true,
                    request_id: Some(task.remote_id.clone()),
                    provider: None,
                    details: error.details.clone(),
                },
                created_at: Utc::now(),
            })
            .await?;
    }
    Ok(())
}

fn remote_poll_backoff(failures: u64) -> Duration {
    let exponent = failures.saturating_sub(1).min(6) as u32;
    Duration::seconds((5_i64.saturating_mul(2_i64.pow(exponent))).min(300))
}

async fn adapter_from_domain_profile(
    state: &AppState,
    profile: &domain::ProviderProfile,
) -> CommandResult<Arc<dyn ProviderAdapter>> {
    let kind = match profile.kind {
        domain::ProviderKind::OpenAi => provider::ProviderKind::OpenAi,
        domain::ProviderKind::XAi => provider::ProviderKind::Xai,
        domain::ProviderKind::Gemini => provider::ProviderKind::Gemini,
        domain::ProviderKind::OpenAiCompatible => provider::ProviderKind::OpenAiCompatible,
    };
    let auth = match &profile.auth_scheme {
        domain::AuthScheme::Bearer => provider::AuthScheme::Bearer,
        domain::AuthScheme::Header { name, prefix } => provider::AuthScheme::Header {
            name: name.clone(),
            prefix: prefix.clone(),
        },
        domain::AuthScheme::QueryParameter { name } => {
            provider::AuthScheme::Query { name: name.clone() }
        }
        domain::AuthScheme::None => {
            return Err(CommandError::validation(
                "provider profile has no API authentication scheme",
            ));
        }
    };
    let mut headers = profile.custom_headers.clone();
    for (name, reference) in &profile.secret_header_refs {
        let key = credential_key_from_reference(reference)?;
        headers.insert(name.clone(), state.secret(key).await?);
    }
    let api_key = state
        .secret(crate::security::CredentialKey::provider(&profile.id))
        .await?;
    let adapter = create_adapter(
        provider::ProviderConfig {
            id: profile.id.clone(),
            name: profile.name.clone(),
            kind,
            base_url: profile.base_url.clone(),
            auth,
            headers,
            timeout_secs: profile.timeout_ms.saturating_add(999) / 1_000,
            proxy_url: profile.proxy_url.clone(),
            organization: profile.organization.clone(),
            project: profile.project.clone(),
            api_version: profile.api_version.clone(),
            models_path: profile.models_path.clone(),
        },
        provider::ProviderCredentials { api_key },
    )?;
    Ok(Arc::from(adapter))
}

fn credential_key_from_reference(reference: &str) -> CommandResult<crate::security::CredentialKey> {
    let value = reference
        .strip_prefix("keyring://")
        .ok_or_else(|| CommandError::validation("invalid keyring reference"))?;
    let (service, account) = value
        .split_once('/')
        .ok_or_else(|| CommandError::validation("invalid keyring reference"))?;
    Ok(crate::security::CredentialKey {
        service: service.to_owned(),
        account: account.to_owned(),
    })
}

fn parse_remote_status(status: &str) -> provider::RemoteJobStatus {
    match status {
        "queued" => provider::RemoteJobStatus::Queued,
        "running" => provider::RemoteJobStatus::Running,
        "succeeded" => provider::RemoteJobStatus::Succeeded,
        "failed" => provider::RemoteJobStatus::Failed,
        "cancelled" => provider::RemoteJobStatus::Cancelled,
        "expired" => provider::RemoteJobStatus::Expired,
        _ => provider::RemoteJobStatus::Unknown,
    }
}

async fn execute_batch(
    state: &AppState,
    project: &ProjectStore,
    adapter: Arc<dyn ProviderAdapter>,
    frontend: &GenerateImagesRequest,
    prepared: &PreparedGenerationRequest,
    run: &mut domain::RunRecord,
    token: CancellationToken,
) -> CommandResult<GenerationCommandResult> {
    adapter.validate(&prepared.provider_request)?;
    let now = Utc::now();
    let mut job = domain_job(run, &prepared.domain_request, 0, now);
    project.upsert_job(&job).await?;
    let submission = build_batch_submission(&prepared.provider_request, frontend.draft.count)?;
    let queue_gate = state.queue_gate();
    let limit = state.provider_limit(&frontend.provider.id).await;
    let permit = match acquire_queued_permit(&token, &queue_gate, limit).await {
        Ok(permit) => permit,
        Err(error) => {
            persist_failure(project, run, &mut job, &error).await?;
            return Err(error.into());
        }
    };
    job.status = domain::JobStatus::Submitting;
    project.upsert_job(&job).await?;
    set_run_running(project, run).await?;
    if !queue_gate.wait(&token).await {
        let error = cancelled_error();
        drop(permit);
        persist_failure(project, run, &mut job, &error).await?;
        return Err(error.into());
    }
    let remote = match tokio::select! {
        _ = token.cancelled() => Err(cancelled_error()),
        remote = adapter.submit_batch(&submission) => remote,
    } {
        Ok(remote) => remote,
        Err(error) => {
            drop(permit);
            persist_failure(project, run, &mut job, &error).await?;
            return Err(error.into());
        }
    };
    drop(permit);
    job.status = domain::JobStatus::WaitingRemote;
    job.remote_batch_id = Some(remote.id.clone());
    job.next_poll_at = Some(Utc::now() + Duration::seconds(2));
    job.updated_at = Utc::now();
    project.upsert_job(&job).await?;
    persist_remote_task(project, run, Some(&job), &frontend.provider.id, &remote).await?;
    run.provider_request_id = Some(remote.id.clone());
    run.redacted_response = Some(sanitize_provider_json(&remote.raw));
    run.updated_at = Utc::now();
    project.upsert_run(run).await?;
    let usage = UsageDto::default();
    Ok(GenerationCommandResult {
        run_id: run.id.clone(),
        request_id: remote.id.clone(),
        interaction_id: None,
        failure_reason: None,
        assets: Vec::new(),
        response_parts: vec![
            remote_job_part(&remote),
            ResponsePartDto::Usage {
                id: Uuid::new_v4().to_string(),
                usage: usage.clone(),
            },
            ResponsePartDto::RequestMeta {
                id: Uuid::new_v4().to_string(),
                request_id: remote.id.clone(),
                interaction_id: None,
                provider_response_id: Some(remote.id.clone()),
            },
        ],
        usage,
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute_realtime(
    state: &AppState,
    project: &ProjectStore,
    adapter: Arc<dyn ProviderAdapter>,
    frontend: &GenerateImagesRequest,
    prepared: &PreparedGenerationRequest,
    run: &mut domain::RunRecord,
    token: CancellationToken,
    provider_kind: provider::ProviderKind,
    event_handler: Option<provider::EventHandler>,
) -> CommandResult<GenerationCommandResult> {
    set_run_running(project, run).await?;
    let split_provider_requests = provider_kind == provider::ProviderKind::Gemini
        || prepared
            .provider_request
            .options
            .openai
            .as_ref()
            .is_some_and(|options| options.api_surface == provider::OpenAiApiSurface::Responses);
    let split_count = if split_provider_requests {
        frontend.draft.count
    } else {
        1
    };
    let limit = state.provider_limit(&frontend.provider.id).await;
    let queue_gate = state.queue_gate();
    let mut futures = FuturesUnordered::new();
    for sequence in 0..split_count {
        let mut provider_request = prepared.provider_request.clone();
        provider_request.request_id = format!("{}-{sequence}", run.id);
        if split_provider_requests {
            provider_request.output.count = 1;
        }
        let mut domain_request = prepared.domain_request.clone();
        domain_request.id.clone_from(&provider_request.request_id);
        if split_provider_requests {
            domain_request.output.count = 1;
        }
        let job = domain_job(run, &domain_request, sequence, Utc::now());
        project.upsert_job(&job).await?;
        let adapter = Arc::clone(&adapter);
        let limit = Arc::clone(&limit);
        let queue_gate = Arc::clone(&queue_gate);
        let token = token.clone();
        let stream = frontend.draft.stream;
        let event_handler = event_handler.clone();
        futures.push(async move {
            let (result, attempts) = execute_provider_request(
                adapter,
                provider_request,
                stream,
                limit,
                queue_gate,
                token,
                event_handler,
            )
            .await;
            (job, attempts, result)
        });
    }

    let mut result = GenerationCommandResult::default();
    let mut failures = Vec::new();
    let mut raw_responses = Vec::new();
    let mut has_pending = false;
    let mut has_successful_output = false;
    let mut output_sequence = 0_u32;
    while let Some((mut job, attempts, response)) = futures.next().await {
        job.attempt = attempts;
        match response {
            Ok(response) => {
                record_continuation_id(run, &response.raw);
                raw_responses.push(sanitize_provider_json(&response.raw));
                run.provider_request_id.clone_from(&response.request_id);
                run.model_version.clone_from(&response.model);
                if let Some(remote) = &response.remote_job {
                    let remote_pending = matches!(
                        remote.status,
                        provider::RemoteJobStatus::Queued | provider::RemoteJobStatus::Running
                    );
                    has_pending |= remote_pending;
                    if remote_pending {
                        job.status = domain::JobStatus::WaitingRemote;
                        job.remote_job_id = Some(remote.id.clone());
                        job.next_poll_at = Some(Utc::now() + Duration::seconds(2));
                        persist_remote_task(
                            project,
                            run,
                            Some(&job),
                            &frontend.provider.id,
                            remote,
                        )
                        .await?;
                        result.response_parts.push(remote_job_part(remote));
                    }
                }
                let processed = process_response(
                    project,
                    Arc::clone(&adapter),
                    &frontend.provider.id,
                    run,
                    &job,
                    response,
                    &mut output_sequence,
                )
                .await?;
                has_successful_output |= response_has_content(&processed.result);
                failures.extend(processed.failures);
                if job.status != domain::JobStatus::WaitingRemote
                    && response_has_content(&processed.result)
                {
                    job.status = domain::JobStatus::Succeeded;
                } else if job.status != domain::JobStatus::WaitingRemote {
                    job.status = domain::JobStatus::Failed;
                }
                merge_result(&mut result, processed.result);
                job.updated_at = Utc::now();
                project.upsert_job(&job).await?;
            }
            Err(error) => {
                persist_failure(project, run, &mut job, &error).await?;
                failures.push(error);
            }
        }
    }
    run.redacted_response = Some(Value::Array(raw_responses));
    run.updated_at = Utc::now();
    let all_failures_cancelled = !failures.is_empty()
        && failures
            .iter()
            .all(|error| error.kind == ProviderErrorKind::Cancelled);
    run.status = if failures.is_empty() {
        if has_pending {
            domain::RunStatus::Running
        } else {
            domain::RunStatus::Succeeded
        }
    } else if !has_successful_output && !has_pending {
        if all_failures_cancelled {
            domain::RunStatus::Cancelled
        } else {
            domain::RunStatus::Failed
        }
    } else {
        domain::RunStatus::PartiallySucceeded
    };
    if !has_pending {
        run.finished_at = Some(Utc::now());
    }
    project.upsert_run(run).await?;
    if !failures.is_empty() {
        result.failure_reason = Some(
            failures
                .iter()
                .map(|error| error.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
    if matches!(
        run.status,
        domain::RunStatus::Failed | domain::RunStatus::Cancelled
    ) {
        return Err(failures
            .into_iter()
            .next()
            .map(CommandError::from)
            .unwrap_or_else(|| CommandError::new("generation_failed", "generation failed")));
    }
    finalize_result(&mut result, &run.id);
    Ok(result)
}

async fn execute_provider_request(
    adapter: Arc<dyn ProviderAdapter>,
    request: provider::GenerationRequest,
    stream: bool,
    limit: Arc<Semaphore>,
    queue_gate: Arc<QueueGate>,
    token: CancellationToken,
    event_handler: Option<provider::EventHandler>,
) -> (Result<provider::GenerationResponse, ProviderError>, u16) {
    if let Err(error) = adapter.validate(&request) {
        return (Err(error), 0);
    }
    const MAX_ATTEMPTS: u16 = 3;
    for attempt in 1..=MAX_ATTEMPTS {
        let result = execute_provider_attempt(
            Arc::clone(&adapter),
            &request,
            stream,
            Arc::clone(&limit),
            &queue_gate,
            &token,
            event_handler.clone(),
        )
        .await;
        match result {
            Ok(response) => return (Ok(response), attempt),
            Err(error) if attempt < MAX_ATTEMPTS && should_retry(&error) => {
                let delay = retry_delay(&error, attempt);
                tokio::select! {
                    _ = token.cancelled() => return (Err(cancelled_error()), attempt),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            Err(error) => return (Err(error), attempt),
        }
    }
    (Err(cancelled_error()), MAX_ATTEMPTS)
}

async fn execute_provider_attempt(
    adapter: Arc<dyn ProviderAdapter>,
    request: &provider::GenerationRequest,
    stream: bool,
    limit: Arc<Semaphore>,
    queue_gate: &QueueGate,
    token: &CancellationToken,
    event_handler: Option<provider::EventHandler>,
) -> Result<provider::GenerationResponse, ProviderError> {
    let permit = acquire_queued_permit(token, queue_gate, limit).await?;
    let result = if stream {
        let events: provider::EventHandler = event_handler.unwrap_or_else(|| Arc::new(|_| {}));
        tokio::select! {
            _ = token.cancelled() => Err(cancelled_error()),
            result = adapter.execute_stream(request, events) => result,
        }
    } else {
        tokio::select! {
            _ = token.cancelled() => Err(cancelled_error()),
            result = adapter.execute(request) => result,
        }
    };
    drop(permit);
    result
}

async fn acquire_queued_permit(
    token: &CancellationToken,
    queue_gate: &QueueGate,
    limit: Arc<Semaphore>,
) -> Result<tokio::sync::OwnedSemaphorePermit, ProviderError> {
    if !queue_gate.wait(token).await {
        return Err(cancelled_error());
    }
    let permit = tokio::select! {
        _ = token.cancelled() => return Err(cancelled_error()),
        permit = limit.acquire_owned() => permit.map_err(|_| cancelled_error())?,
    };
    if !queue_gate.wait(token).await {
        drop(permit);
        return Err(cancelled_error());
    }
    Ok(permit)
}

fn should_retry(error: &ProviderError) -> bool {
    matches!(
        error.kind,
        ProviderErrorKind::RateLimit | ProviderErrorKind::Http | ProviderErrorKind::Io
    ) || error.status.is_some_and(|status| status >= 500)
}

fn retry_delay(error: &ProviderError, attempt: u16) -> std::time::Duration {
    if let Some(seconds) = error.retry_after_seconds {
        return std::time::Duration::from_secs(seconds.clamp(1, 30));
    }
    let exponential_ms = 500_u64.saturating_mul(1_u64 << u32::from(attempt.saturating_sub(1)));
    let jitter_ms = u64::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_millis()
            % 251,
    );
    std::time::Duration::from_millis((exponential_ms + jitter_ms).min(5_000))
}

async fn execute_cancellable<T>(
    token: &CancellationToken,
    limit: Arc<Semaphore>,
    future: impl std::future::Future<Output = Result<T, ProviderError>>,
) -> Result<T, ProviderError> {
    let permit = tokio::select! {
        _ = token.cancelled() => return Err(cancelled_error()),
        permit = limit.acquire_owned() => permit.map_err(|_| cancelled_error())?,
    };
    let result = tokio::select! {
        _ = token.cancelled() => Err(cancelled_error()),
        result = future => result,
    };
    drop(permit);
    result
}

async fn process_response(
    project: &ProjectStore,
    adapter: Arc<dyn ProviderAdapter>,
    provider_profile_id: &str,
    run: &domain::RunRecord,
    job: &domain::JobRecord,
    response: provider::GenerationResponse,
    sequence: &mut u32,
) -> CommandResult<ProcessedResponse> {
    let interaction_id = response
        .raw
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| id.contains("interaction") || id.starts_with("resp_"))
        .map(ToOwned::to_owned);
    let mut result = GenerationCommandResult {
        run_id: run.id.clone(),
        request_id: response
            .request_id
            .clone()
            .unwrap_or_else(|| run.id.clone()),
        interaction_id,
        ..GenerationCommandResult::default()
    };
    let mut failures = Vec::new();
    if let Some(remote) = &response.remote_job {
        result.response_parts.push(remote_job_part(remote));
    }
    for output in response.outputs {
        let current = *sequence;
        *sequence += 1;
        match output {
            provider::OutputPart::Image {
                source,
                mime_type,
                revised_prompt,
                remote_file,
            } => {
                let downloaded = match adapter.download_asset(&source).await {
                    Ok(downloaded) => downloaded,
                    Err(error) => {
                        save_provider_error(project, run, Some(job), &error).await?;
                        failures.push(error);
                        continue;
                    }
                };
                let mime = downloaded
                    .mime_type
                    .or(mime_type)
                    .unwrap_or_else(|| "image/png".to_owned());
                let extension = extension_for_output(&mime, downloaded.filename.as_deref());
                let directory = project
                    .layout()
                    .output_run_directory(Utc::now().date_naive(), &run.id)?;
                let path = directory.join(format!("{:03}.{}", current + 1, extension));
                let stored = atomic_write_new(&path, &downloaded.bytes)?;
                let dimensions = image::load_from_memory(&downloaded.bytes)
                    .map(|image| (image.width(), image.height()))
                    .unwrap_or((0, 0));
                let asset = GeneratedAsset {
                    id: Uuid::new_v4().to_string(),
                    task_id: run.id.clone(),
                    url: preview_data_url(&mime, &downloaded.bytes),
                    file_path: stored.path.to_string_lossy().into_owned(),
                    width: dimensions.0,
                    height: dimensions.1,
                    format: extension.to_owned(),
                    prompt: run.final_prompt.clone(),
                    created_at: Utc::now().to_rfc3339(),
                    selected: None,
                };
                let provider_file_id = remote_file.as_ref().map(|file| file.id.clone());
                let domain_output = domain::OutputPart {
                    id: asset.id.clone(),
                    run_id: run.id.clone(),
                    job_id: Some(job.id.clone()),
                    sequence: current,
                    kind: domain::OutputPartKind::Image,
                    text: revised_prompt.clone(),
                    local_path: Some(
                        stored
                            .path
                            .strip_prefix(project.layout().root())
                            .unwrap_or(&stored.path)
                            .to_owned(),
                    ),
                    remote_url: match &source {
                        provider::AssetSource::Url { url } => Some(url.clone()),
                        _ => None,
                    },
                    provider_file_id: provider_file_id.clone(),
                    mime_type: Some(mime.clone()),
                    sha256: Some(stored.sha256),
                    size_bytes: Some(stored.size_bytes),
                    metadata: BTreeMap::new(),
                    created_at: Utc::now(),
                };
                project.upsert_output(&domain_output).await?;
                if let Some(file) = remote_file {
                    let remote_part = ResponsePartDto::RemoteFile {
                        id: Uuid::new_v4().to_string(),
                        name: file.filename.clone().unwrap_or_else(|| file.id.clone()),
                        uri: file.public_url.clone().unwrap_or_else(|| file.id.clone()),
                        mime_type: mime.clone(),
                        size_bytes: Some(stored.size_bytes),
                    };
                    persist_remote_file(project, provider_profile_id, file).await?;
                    result.response_parts.push(remote_part);
                }
                result.assets.push(asset.clone());
                result.response_parts.push(ResponsePartDto::Image {
                    id: Uuid::new_v4().to_string(),
                    asset_id: asset.id.clone(),
                    url: asset.url.clone(),
                    mime_type: mime,
                    width: asset.width,
                    height: asset.height,
                    file_path: asset.file_path.clone(),
                });
            }
            provider::OutputPart::Text { text, annotations } => {
                persist_text_output(
                    project,
                    run,
                    job,
                    current,
                    domain::OutputPartKind::Text,
                    &text,
                    json!({ "annotations": annotations }),
                )
                .await?;
                result.response_parts.push(ResponsePartDto::Text {
                    id: Uuid::new_v4().to_string(),
                    text,
                });
            }
            provider::OutputPart::Thought { text, signature } => {
                persist_text_output(
                    project,
                    run,
                    job,
                    current,
                    domain::OutputPartKind::Thought,
                    &text,
                    json!({ "signature": signature }),
                )
                .await?;
                result.response_parts.push(ResponsePartDto::Thought {
                    id: Uuid::new_v4().to_string(),
                    summary: text,
                    image_url: None,
                });
            }
            provider::OutputPart::Citation {
                title,
                url,
                snippet,
                raw,
            } => {
                persist_text_output(
                    project,
                    run,
                    job,
                    current,
                    domain::OutputPartKind::Citation,
                    snippet.as_deref().unwrap_or_default(),
                    sanitize_provider_json(&raw),
                )
                .await?;
                result.response_parts.push(ResponsePartDto::Citation {
                    id: Uuid::new_v4().to_string(),
                    title,
                    url,
                    snippet,
                    start_index: raw
                        .get("start_index")
                        .or_else(|| raw.get("startIndex"))
                        .and_then(Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok()),
                    end_index: raw
                        .get("end_index")
                        .or_else(|| raw.get("endIndex"))
                        .and_then(Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok()),
                });
            }
            provider::OutputPart::SearchSuggestions { html, signature } => {
                persist_text_output(
                    project,
                    run,
                    job,
                    current,
                    domain::OutputPartKind::SearchSuggestion,
                    &html,
                    json!({ "signature": signature }),
                )
                .await?;
                result
                    .response_parts
                    .push(ResponsePartDto::SearchSuggestions {
                        id: Uuid::new_v4().to_string(),
                        html,
                    });
            }
        }
    }
    if let Some(usage) = response.usage {
        let domain_usage = domain::UsageRecord {
            id: Uuid::new_v4().to_string(),
            run_id: run.id.clone(),
            job_id: Some(job.id.clone()),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            image_count: Some(result.assets.len() as u64),
            input_bytes: None,
            output_bytes: None,
            cost_micros: usage
                .cost_usd
                .map(|cost| (cost * 1_000_000.0).round() as u64),
            currency: usage.cost_usd.map(|_| "USD".to_owned()),
            details: usage
                .raw
                .as_ref()
                .map(sanitize_provider_json)
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default()
                .into_iter()
                .collect(),
            created_at: Utc::now(),
        };
        project.save_usage(&domain_usage).await?;
        result.usage = UsageDto {
            input_tokens: usage.input_tokens.unwrap_or_default(),
            output_tokens: usage.output_tokens.unwrap_or_default(),
            thought_tokens: usage
                .raw
                .as_ref()
                .and_then(|raw| {
                    raw.get("thought_tokens")
                        .or_else(|| raw.get("thoughtsTokenCount"))
                })
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            cached_tokens: usage
                .raw
                .as_ref()
                .and_then(|raw| {
                    raw.get("cached_tokens")
                        .or_else(|| raw.get("cachedContentTokenCount"))
                })
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            total_tokens: usage.total_tokens.unwrap_or_default(),
            generated_images: result.assets.len() as u64,
            image_tokens: usage.image_tokens,
            cost_usd: usage.cost_usd,
        };
    }
    Ok(ProcessedResponse { result, failures })
}

struct ProcessedResponse {
    result: GenerationCommandResult,
    failures: Vec<ProviderError>,
}

fn response_has_content(result: &GenerationCommandResult) -> bool {
    !result.assets.is_empty()
        || result.response_parts.iter().any(|part| {
            matches!(
                part,
                ResponsePartDto::Text { .. }
                    | ResponsePartDto::Thought { .. }
                    | ResponsePartDto::Citation { .. }
                    | ResponsePartDto::SearchSuggestions { .. }
            )
        })
}

fn record_continuation_id(run: &mut domain::RunRecord, raw: &Value) {
    if let Some(id) = raw
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| id.contains("interaction") || id.starts_with("resp_"))
    {
        run.request
            .metadata
            .insert("continuationId".to_owned(), Value::String(id.to_owned()));
    }
}

async fn persist_text_output(
    project: &ProjectStore,
    run: &domain::RunRecord,
    job: &domain::JobRecord,
    sequence: u32,
    kind: domain::OutputPartKind,
    text: &str,
    metadata: Value,
) -> CommandResult<()> {
    project
        .upsert_output(&domain::OutputPart {
            id: Uuid::new_v4().to_string(),
            run_id: run.id.clone(),
            job_id: Some(job.id.clone()),
            sequence,
            kind,
            text: Some(text.to_owned()),
            local_path: None,
            remote_url: None,
            provider_file_id: None,
            mime_type: None,
            sha256: None,
            size_bytes: None,
            metadata: metadata
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            created_at: Utc::now(),
        })
        .await?;
    Ok(())
}

async fn persist_inputs(
    project: &ProjectStore,
    prepared: &PreparedGenerationRequest,
    run_id: &str,
) -> CommandResult<()> {
    for input in &prepared.input_assets {
        let mut persisted = input.clone();
        persisted.id = format!("{run_id}:{}", input.id);
        project
            .save_input_asset(&persisted, Some(run_id), None)
            .await?;
    }
    if let Some(mask) = &prepared.mask_asset {
        let mut persisted = mask.clone();
        persisted.id = format!("{run_id}:{}", mask.id);
        project
            .save_input_asset(&persisted, Some(run_id), None)
            .await?;
    }
    Ok(())
}

async fn persist_failure(
    project: &ProjectStore,
    run: &mut domain::RunRecord,
    job: &mut domain::JobRecord,
    error: &ProviderError,
) -> CommandResult<()> {
    job.status = if error.kind == ProviderErrorKind::Cancelled {
        domain::JobStatus::Cancelled
    } else {
        domain::JobStatus::Failed
    };
    job.updated_at = Utc::now();
    project.upsert_job(job).await?;
    save_provider_error(project, run, Some(job), error).await?;
    run.status = if error.kind == ProviderErrorKind::Cancelled {
        domain::RunStatus::Cancelled
    } else {
        domain::RunStatus::Failed
    };
    run.finished_at = Some(Utc::now());
    run.updated_at = Utc::now();
    project.upsert_run(run).await?;
    Ok(())
}

async fn save_provider_error(
    project: &ProjectStore,
    run: &domain::RunRecord,
    job: Option<&domain::JobRecord>,
    error: &ProviderError,
) -> CommandResult<()> {
    let details = error.details.as_ref().map(sanitize_provider_json);
    project
        .save_error(&domain::ErrorRecord {
            id: Uuid::new_v4().to_string(),
            run_id: run.id.clone(),
            job_id: job.map(|job| job.id.clone()),
            error: domain::ProviderError {
                code: error
                    .code
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", error.kind).to_ascii_lowercase()),
                message: error.message.clone(),
                http_status: error.status,
                retryable: error.kind == ProviderErrorKind::RateLimit
                    || error.kind == ProviderErrorKind::Http
                    || error.status.is_some_and(|status| status >= 500),
                request_id: error.request_id.clone(),
                provider: None,
                details,
            },
            created_at: Utc::now(),
        })
        .await?;
    Ok(())
}

async fn persist_remote_task(
    project: &ProjectStore,
    run: &domain::RunRecord,
    job: Option<&domain::JobRecord>,
    provider_profile_id: &str,
    remote: &provider::RemoteJob,
) -> CommandResult<()> {
    project
        .upsert_remote_task(&RemoteTaskRecord {
            id: Uuid::new_v4().to_string(),
            run_id: run.id.clone(),
            job_id: job.map(|job| job.id.clone()),
            provider_profile_id: provider_profile_id.to_owned(),
            remote_id: remote.id.clone(),
            task_type: remote_job_kind(remote.kind).to_owned(),
            status: remote_job_status(remote.status).to_owned(),
            next_poll_at: Some(Utc::now() + Duration::seconds(2)),
            metadata: sanitize_provider_json(&remote.raw)
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .await?;
    Ok(())
}

async fn persist_remote_file(
    project: &ProjectStore,
    provider_profile_id: &str,
    file: provider::RemoteFile,
) -> CommandResult<()> {
    project
        .upsert_remote_file(&RemoteFileRecord {
            id: Uuid::new_v4().to_string(),
            provider_profile_id: provider_profile_id.to_owned(),
            provider_file_id: file.id,
            purpose: file.filename,
            expires_at: file.expires_at.as_ref().and_then(parse_provider_timestamp),
            metadata: BTreeMap::from([
                (
                    "publicUrl".to_owned(),
                    file.public_url.map(Value::String).unwrap_or(Value::Null),
                ),
                (
                    "publicUrlExpiresAt".to_owned(),
                    file.public_url_expires_at.unwrap_or(Value::Null),
                ),
                (
                    "publicUrlError".to_owned(),
                    file.public_url_error
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                ),
            ]),
            created_at: Utc::now(),
        })
        .await?;
    Ok(())
}

fn parse_provider_timestamp(value: &Value) -> Option<chrono::DateTime<Utc>> {
    value
        .as_i64()
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.with_timezone(&Utc))
        })
}

fn domain_job(
    run: &domain::RunRecord,
    request: &domain::GenerationRequest,
    sequence: u16,
    now: chrono::DateTime<Utc>,
) -> domain::JobRecord {
    domain::JobRecord {
        id: Uuid::new_v4().to_string(),
        run_id: run.id.clone(),
        sequence,
        status: domain::JobStatus::Queued,
        attempt: 0,
        remote_job_id: None,
        remote_batch_id: None,
        next_poll_at: None,
        request: request.clone(),
        created_at: now,
        updated_at: now,
    }
}

async fn set_run_running(project: &ProjectStore, run: &mut domain::RunRecord) -> CommandResult<()> {
    run.status = domain::RunStatus::Running;
    run.started_at = Some(Utc::now());
    run.updated_at = Utc::now();
    project.upsert_run(run).await?;
    Ok(())
}

fn merge_result(target: &mut GenerationCommandResult, source: GenerationCommandResult) {
    if !source.run_id.is_empty() {
        target.run_id = source.run_id;
    }
    target.assets.extend(source.assets);
    target.response_parts.extend(source.response_parts);
    if !source.request_id.is_empty() {
        target.request_id = source.request_id;
    }
    target.interaction_id = source
        .interaction_id
        .or_else(|| target.interaction_id.take());
    target.usage.input_tokens = target
        .usage
        .input_tokens
        .saturating_add(source.usage.input_tokens);
    target.usage.output_tokens = target
        .usage
        .output_tokens
        .saturating_add(source.usage.output_tokens);
    target.usage.thought_tokens = target
        .usage
        .thought_tokens
        .saturating_add(source.usage.thought_tokens);
    target.usage.cached_tokens = target
        .usage
        .cached_tokens
        .saturating_add(source.usage.cached_tokens);
    target.usage.total_tokens = target
        .usage
        .total_tokens
        .saturating_add(source.usage.total_tokens);
    target.usage.generated_images = target
        .usage
        .generated_images
        .saturating_add(source.usage.generated_images);
    target.usage.image_tokens = add_options(target.usage.image_tokens, source.usage.image_tokens);
    target.usage.cost_usd = match (target.usage.cost_usd, source.usage.cost_usd) {
        (Some(left), Some(right)) => Some(left + right),
        (left, right) => left.or(right),
    };
}

fn add_options(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (left, right) => left.or(right),
    }
}

fn remote_job_part(remote: &provider::RemoteJob) -> ResponsePartDto {
    ResponsePartDto::RemoteJob {
        id: Uuid::new_v4().to_string(),
        job_id: remote.id.clone(),
        kind: remote_job_kind(remote.kind).to_owned(),
        status: remote_job_status(remote.status).to_owned(),
        provider: match remote.provider {
            provider::ProviderKind::OpenAi => "openai",
            provider::ProviderKind::Xai => "xai",
            provider::ProviderKind::Gemini => "gemini",
            provider::ProviderKind::OpenAiCompatible => "custom",
        }
        .to_owned(),
        model: remote.model.clone(),
    }
}

fn finalize_result(result: &mut GenerationCommandResult, fallback_request_id: &str) {
    if result.run_id.is_empty() {
        result.run_id = fallback_request_id.to_owned();
    }
    if result.request_id.is_empty() {
        result.request_id = fallback_request_id.to_owned();
    }
    result.response_parts.push(ResponsePartDto::Usage {
        id: Uuid::new_v4().to_string(),
        usage: result.usage.clone(),
    });
    result.response_parts.push(ResponsePartDto::RequestMeta {
        id: Uuid::new_v4().to_string(),
        request_id: result.request_id.clone(),
        interaction_id: result.interaction_id.clone(),
        provider_response_id: Some(result.request_id.clone()),
    });
}

fn remote_job_kind(kind: provider::RemoteJobKind) -> &'static str {
    match kind {
        provider::RemoteJobKind::Background => "background",
        provider::RemoteJobKind::Batch => "batch",
    }
}

fn remote_job_status(status: provider::RemoteJobStatus) -> &'static str {
    match status {
        provider::RemoteJobStatus::Queued => "queued",
        provider::RemoteJobStatus::Running => "running",
        provider::RemoteJobStatus::Succeeded => "succeeded",
        provider::RemoteJobStatus::Failed => "failed",
        provider::RemoteJobStatus::Cancelled => "cancelled",
        provider::RemoteJobStatus::Expired => "expired",
        provider::RemoteJobStatus::Unknown => "unknown",
    }
}

fn cancelled_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Cancelled, "generation was cancelled")
}

fn extension_for_output<'a>(mime: &str, filename: Option<&'a str>) -> &'a str {
    if let Some(extension) = filename
        .and_then(|filename| std::path::Path::new(filename).extension())
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
    {
        return extension;
    }
    match mime
        .to_ascii_lowercase()
        .split(';')
        .next()
        .unwrap_or_default()
    {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    }
}

fn preview_data_url(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

impl From<serde_json::Error> for CommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::new("serialization", error.to_string())
    }
}

impl From<StorageError> for ProviderError {
    fn from(error: StorageError) -> Self {
        ProviderError::io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_renderable_preview_urls_without_exposing_paths() {
        let url = preview_data_url("image/png", b"png");
        assert_eq!(url, "data:image/png;base64,cG5n");
        assert!(!url.contains("C:/"));
    }

    #[test]
    fn aggregates_optional_usage() {
        assert_eq!(add_options(Some(2), Some(3)), Some(5));
        assert_eq!(add_options(None, Some(3)), Some(3));
    }

    #[test]
    fn retries_only_transient_provider_failures() {
        let rate_limit = ProviderError::new(ProviderErrorKind::RateLimit, "slow down");
        let validation = ProviderError::new(ProviderErrorKind::Validation, "bad size");
        let authentication = ProviderError::new(ProviderErrorKind::Authentication, "bad key");

        assert!(should_retry(&rate_limit));
        assert!(!should_retry(&validation));
        assert!(!should_retry(&authentication));
        assert!(retry_delay(&rate_limit, 2) >= std::time::Duration::from_millis(1_000));
    }

    #[test]
    fn remote_poll_backoff_is_exponential_and_bounded() {
        assert_eq!(remote_poll_backoff(1), Duration::seconds(5));
        assert_eq!(remote_poll_backoff(3), Duration::seconds(20));
        assert_eq!(remote_poll_backoff(100), Duration::seconds(300));
    }

    #[test]
    fn partial_success_requires_a_real_output_part() {
        let mut result = GenerationCommandResult::default();
        result.response_parts.push(ResponsePartDto::Usage {
            id: "usage".to_owned(),
            usage: UsageDto::default(),
        });
        assert!(!response_has_content(&result));
        result.response_parts.push(ResponsePartDto::Text {
            id: "text".to_owned(),
            text: "done".to_owned(),
        });
        assert!(response_has_content(&result));
    }

    #[tokio::test]
    async fn queued_permit_rechecks_pause_after_waiting_for_a_provider_slot() {
        let gate = Arc::new(QueueGate::default());
        let limit = Arc::new(Semaphore::new(0));
        let token = CancellationToken::new();
        let waiter = tokio::spawn({
            let gate = Arc::clone(&gate);
            let limit = Arc::clone(&limit);
            async move { acquire_queued_permit(&token, &gate, limit).await }
        });

        tokio::task::yield_now().await;
        gate.pause();
        limit.add_permits(1);
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        gate.resume();
        assert!(waiter.await.unwrap().is_ok());
    }
}
