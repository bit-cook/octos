use crate::workflow_runtime::{WorkflowInstance, WorkflowKind};
use octos_agent::{FirstPartyHarnessManifest, WorkspacePolicy};

pub fn build() -> WorkflowInstance {
    super::build_first_party_workflow(WorkflowKind::Slides, FirstPartyHarnessManifest::slides())
}

pub fn workspace_policy() -> WorkspacePolicy {
    FirstPartyHarnessManifest::slides().workspace_policy()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_slides_workflow_uses_presentation_output_contract() {
        let workflow = build();
        assert_eq!(workflow.kind, WorkflowKind::Slides);
        assert_eq!(workflow.current_phase.as_str(), "design");
        assert_eq!(
            workflow.terminal_output.required_artifact_kind,
            "presentation"
        );
        assert!(workflow.terminal_output.deliver_final_artifact_only);
        assert!(!workflow.terminal_output.deliver_media_only);
        assert!(workflow.terminal_output.forbid_intermediate_files);
        assert!(
            workflow
                .allowed_tools
                .iter()
                .any(|tool| tool == "mofa_slides")
        );
        assert!(
            workflow
                .allowed_tools
                .iter()
                .any(|tool| tool == "check_workspace_contract")
        );
    }

    #[test]
    fn slides_workspace_policy_is_standardized() {
        let policy = workspace_policy();
        assert_eq!(
            policy.workspace.kind,
            octos_agent::WorkspacePolicyKind::Slides
        );
        assert!(
            policy
                .validation
                .on_turn_end
                .contains(&"file_exists:script.js".to_string())
        );
        assert!(
            policy
                .validation
                .on_completion
                .contains(&"file_exists:output/*.pptx".to_string())
        );
    }

    #[test]
    fn slides_workflow_uses_first_party_harness_terminal_output() {
        let workflow = build();
        let harness = FirstPartyHarnessManifest::slides();

        assert_eq!(
            workflow.terminal_output.required_artifact_kind,
            harness.terminal_output.required_artifact_kind
        );
        assert_eq!(
            workflow.terminal_output.deliver_final_artifact_only,
            harness.terminal_output.deliver_final_artifact_only
        );
    }

    #[test]
    fn slides_workflow_uses_first_party_harness_metadata() {
        let workflow = build();
        let harness = FirstPartyHarnessManifest::slides();

        assert_eq!(workflow.label, harness.workflow.label);
        assert_eq!(workflow.ack_message, harness.workflow.ack_message);
        assert_eq!(
            workflow.current_phase.as_str(),
            harness.workflow.initial_phase
        );
        assert_eq!(workflow.allowed_tools, harness.workflow.allowed_tools);
        assert_eq!(
            workflow.limits.max_dialogue_lines,
            harness.workflow.limits.max_dialogue_lines
        );
        assert_eq!(
            workflow.limits.max_generate_calls,
            harness.workflow.limits.max_generate_calls
        );
        assert_eq!(
            workflow.additional_instructions,
            harness.workflow.additional_instructions
        );
    }
}
