pub mod research_podcast;
pub mod research_report;
pub mod site_delivery;
pub mod slides_delivery;

use octos_agent::{
    FIRST_PARTY_SITES_HARNESS_ID, FIRST_PARTY_SLIDES_HARNESS_ID, ResolvedFirstPartyHarness,
    WorkspacePolicy, resolve_first_party_harness_by_manifest_id,
};

use crate::workflow_runtime::{
    WorkflowInstance, WorkflowKind, WorkflowLimits, WorkflowPhase, WorkflowTerminalOutput,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FirstPartyWorkflowBinding {
    kind: WorkflowKind,
    harness_id: &'static str,
}

const FIRST_PARTY_WORKFLOW_BINDINGS: [FirstPartyWorkflowBinding; 2] = [
    FirstPartyWorkflowBinding {
        kind: WorkflowKind::Slides,
        harness_id: FIRST_PARTY_SLIDES_HARNESS_ID,
    },
    FirstPartyWorkflowBinding {
        kind: WorkflowKind::Site,
        harness_id: FIRST_PARTY_SITES_HARNESS_ID,
    },
];

fn first_party_workflow_binding(kind: WorkflowKind) -> &'static FirstPartyWorkflowBinding {
    FIRST_PARTY_WORKFLOW_BINDINGS
        .iter()
        .find(|binding| binding.kind == kind)
        .unwrap_or_else(|| panic!("workflow {kind:?} does not have a first-party harness"))
}

fn resolve_first_party_workflow_harness(kind: WorkflowKind) -> ResolvedFirstPartyHarness {
    let binding = first_party_workflow_binding(kind);
    resolve_first_party_harness_by_manifest_id(binding.harness_id).unwrap_or_else(|| {
        panic!(
            "missing first-party harness manifest {}",
            binding.harness_id
        )
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_binding_resolves_expected_harness_ids() {
        assert_eq!(
            first_party_workflow_binding(WorkflowKind::Slides).harness_id,
            FIRST_PARTY_SLIDES_HARNESS_ID
        );
        assert_eq!(
            first_party_workflow_binding(WorkflowKind::Site).harness_id,
            FIRST_PARTY_SITES_HARNESS_ID
        );
    }
}
