use crate::workflow_runtime::workflow_families::SiteTemplate;
use crate::workflow_runtime::{WorkflowInstance, WorkflowKind};
use octos_agent::{FirstPartyHarnessManifest, FirstPartyHarnessName, WorkspacePolicy};

pub fn build_output_dir_for_template_kind(template: SiteTemplate) -> &'static str {
    template.output_dir()
}

pub fn build_output_dir_for_template(template: &str) -> &'static str {
    build_output_dir_for_template_kind(SiteTemplate::from_slug(template))
}

pub fn workspace_policy_for_template_kind(template: SiteTemplate) -> WorkspacePolicy {
    FirstPartyHarnessManifest::site_with_build_output(build_output_dir_for_template_kind(template))
        .workspace_policy()
}

pub fn workspace_policy_for_template(template: &str) -> WorkspacePolicy {
    workspace_policy_for_template_kind(SiteTemplate::from_slug(template))
}

pub fn build() -> WorkflowInstance {
    super::build_first_party_workflow(WorkflowKind::Site, FirstPartyHarnessName::Sites.manifest())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_site_workflow_uses_site_output_contract() {
        let workflow = build();
        assert_eq!(workflow.kind, WorkflowKind::Site);
        assert_eq!(workflow.current_phase.as_str(), "scaffold");
        assert_eq!(workflow.terminal_output.required_artifact_kind, "site");
        assert!(workflow.terminal_output.deliver_final_artifact_only);
        assert!(!workflow.terminal_output.deliver_media_only);
        assert!(workflow.terminal_output.forbid_intermediate_files);
        assert!(
            workflow
                .allowed_tools
                .iter()
                .any(|tool| tool == "check_workspace_contract")
        );
    }

    #[test]
    fn build_output_dir_is_template_aware() {
        assert_eq!(build_output_dir_for_template("astro-site"), "dist");
        assert_eq!(build_output_dir_for_template("nextjs-app"), "out");
        assert_eq!(build_output_dir_for_template("react-vite"), "dist");
        assert_eq!(build_output_dir_for_template("other"), "docs");
    }

    #[test]
    fn build_output_dir_is_template_typed() {
        assert_eq!(
            build_output_dir_for_template_kind(SiteTemplate::AstroSite),
            "dist"
        );
        assert_eq!(
            build_output_dir_for_template_kind(SiteTemplate::NextjsApp),
            "out"
        );
        assert_eq!(
            build_output_dir_for_template_kind(SiteTemplate::Docs),
            "docs"
        );
    }

    #[test]
    fn workspace_policy_tracks_template_entrypoint() {
        let policy = workspace_policy_for_template("nextjs-app");
        assert_eq!(
            policy.validation.on_completion,
            vec!["file_exists:out/index.html"]
        );
        assert_eq!(
            policy
                .artifacts
                .entries
                .get("entrypoint")
                .map(String::as_str),
            Some("out/index.html")
        );
    }

    #[test]
    fn site_workflow_uses_first_party_harness_terminal_output() {
        let workflow = build();
        let harness = FirstPartyHarnessName::Sites.manifest();

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
    fn site_workflow_uses_first_party_harness_metadata() {
        let workflow = build();
        let harness = FirstPartyHarnessName::Sites.manifest();

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
