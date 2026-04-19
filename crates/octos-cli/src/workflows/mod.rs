pub mod research_podcast;
pub mod research_report;
pub mod site_delivery;
pub mod slides_delivery;

use octos_agent::{
    ResolvedFirstPartyHarness, WorkspacePolicy, WorkspacePolicyKind,
    resolve_first_party_harness_for_workspace_kind,
};

use crate::workflow_runtime::{
    WorkflowInstance, WorkflowKind, WorkflowLimits, WorkflowPhase, WorkflowTerminalOutput,
};

fn resolve_first_party_workflow_harness(kind: WorkflowKind) -> ResolvedFirstPartyHarness {
    let workspace_kind = match kind {
        WorkflowKind::Slides => WorkspacePolicyKind::Slides,
        WorkflowKind::Site => WorkspacePolicyKind::Sites,
        other => panic!("workflow {other:?} does not have a first-party harness"),
    };

    resolve_first_party_harness_for_workspace_kind(workspace_kind)
        .unwrap_or_else(|| panic!("missing first-party harness for workflow kind {:?}", kind))
}

pub(crate) fn build_first_party_workflow(kind: WorkflowKind) -> WorkflowInstance {
    let harness = resolve_first_party_workflow_harness(kind);
    let workflow = harness.manifest.workflow;
    let terminal_output = harness.manifest.terminal_output;

    WorkflowInstance {
        kind,
        label: workflow.label,
        ack_message: workflow.ack_message,
        current_phase: WorkflowPhase::new(workflow.initial_phase),
        allowed_tools: workflow.allowed_tools,
        limits: WorkflowLimits {
            max_search_passes: workflow.limits.max_search_passes,
            max_pipeline_runs: workflow.limits.max_pipeline_runs,
            max_dialogue_lines: workflow.limits.max_dialogue_lines,
            target_audio_minutes: workflow.limits.target_audio_minutes,
            max_generate_calls: workflow.limits.max_generate_calls,
        },
        terminal_output: WorkflowTerminalOutput {
            deliver_final_artifact_only: terminal_output.deliver_final_artifact_only,
            deliver_media_only: terminal_output.deliver_media_only,
            forbid_intermediate_files: terminal_output.forbid_intermediate_files,
            required_artifact_kind: terminal_output.required_artifact_kind,
        },
        additional_instructions: workflow.additional_instructions,
    }
}

pub(crate) fn workspace_policy_for_first_party_workflow(kind: WorkflowKind) -> WorkspacePolicy {
    resolve_first_party_workflow_harness(kind)
        .manifest
        .workspace_policy()
}
