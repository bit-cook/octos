pub mod research_podcast;
pub mod research_report;
pub mod site_delivery;
pub mod slides_delivery;

use octos_agent::FirstPartyHarnessManifest;

use crate::workflow_runtime::{
    WorkflowInstance, WorkflowKind, WorkflowLimits, WorkflowPhase, WorkflowTerminalOutput,
};

pub(crate) fn build_first_party_workflow(
    kind: WorkflowKind,
    harness: FirstPartyHarnessManifest,
) -> WorkflowInstance {
    let workflow = harness.workflow;
    let terminal_output = harness.terminal_output;

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
