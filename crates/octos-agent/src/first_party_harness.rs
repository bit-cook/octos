use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::workspace_policy::{
    ValidationPolicy, WorkspaceArtifactsPolicy, WorkspacePolicy, WorkspacePolicyWorkspace,
    WorkspaceSpawnTaskPolicy, WorkspaceTrackingPolicy, WorkspaceVersionControlPolicy,
};

const SLIDES_MANIFEST_TOML: &str = include_str!("first_party_harness/slides.toml");
const SITES_MANIFEST_TOML: &str = include_str!("first_party_harness/sites.toml");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstPartyHarnessManifest {
    pub id: String,
    pub workspace: WorkspacePolicyWorkspace,
    pub version_control: WorkspaceVersionControlPolicy,
    pub tracking: WorkspaceTrackingPolicy,
    #[serde(default)]
    pub validation: ValidationPolicy,
    #[serde(default)]
    pub artifacts: WorkspaceArtifactsPolicy,
    #[serde(default)]
    pub spawn_tasks: BTreeMap<String, WorkspaceSpawnTaskPolicy>,
    pub workflow: FirstPartyWorkflowDeclaration,
    pub terminal_output: FirstPartyTerminalOutput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstPartyWorkflowDeclaration {
    pub label: String,
    pub ack_message: String,
    pub initial_phase: String,
    pub allowed_tools: Vec<String>,
    pub limits: FirstPartyWorkflowLimits,
    pub additional_instructions: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstPartyWorkflowLimits {
    #[serde(default)]
    pub max_search_passes: Option<u32>,
    #[serde(default)]
    pub max_pipeline_runs: Option<u32>,
    #[serde(default)]
    pub max_dialogue_lines: Option<u32>,
    #[serde(default)]
    pub target_audio_minutes: Option<u32>,
    #[serde(default)]
    pub max_generate_calls: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstPartyTerminalOutput {
    pub deliver_final_artifact_only: bool,
    pub deliver_media_only: bool,
    pub forbid_intermediate_files: bool,
    pub required_artifact_kind: String,
}

impl FirstPartyHarnessManifest {
    pub fn slides() -> Self {
        bundled_manifest("slides.toml", SLIDES_MANIFEST_TOML)
    }

    pub fn sites() -> Self {
        bundled_manifest("sites.toml", SITES_MANIFEST_TOML)
    }

    pub fn site_with_build_output(build_output_dir: &str) -> Self {
        let mut manifest = Self::sites();
        manifest.id = format!("first_party.sites.{build_output_dir}");
        manifest.validation = ValidationPolicy {
            on_turn_end: vec![
                "file_exists:mofa-site-session.json".into(),
                "file_exists:site-plan.json".into(),
                "file_exists:optimized-prompt.md".into(),
            ],
            on_source_change: Vec::new(),
            on_completion: vec![format!("file_exists:{build_output_dir}/index.html")],
        };
        manifest.artifacts = WorkspaceArtifactsPolicy {
            entries: BTreeMap::from([(
                "entrypoint".into(),
                format!("{build_output_dir}/index.html"),
            )]),
        };
        manifest
    }

    pub fn workspace_policy(&self) -> WorkspacePolicy {
        WorkspacePolicy {
            workspace: self.workspace.clone(),
            version_control: self.version_control.clone(),
            tracking: self.tracking.clone(),
            validation: self.validation.clone(),
            artifacts: self.artifacts.clone(),
            spawn_tasks: self.spawn_tasks.clone(),
        }
    }
}

fn bundled_manifest(name: &str, source: &str) -> FirstPartyHarnessManifest {
    toml::from_str(source).unwrap_or_else(|error| {
        panic!("bundled first-party harness manifest {name} should parse: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkspacePolicyKind;

    #[test]
    fn slides_manifest_declares_expected_contract() {
        let manifest = FirstPartyHarnessManifest::slides();

        assert_eq!(manifest.id, "first_party.slides");
        assert_eq!(manifest.workspace.kind, WorkspacePolicyKind::Slides);
        assert_eq!(
            manifest.terminal_output.required_artifact_kind,
            "presentation"
        );
        assert_eq!(manifest.workflow.label, "Slides deliverable");
        assert_eq!(manifest.workflow.initial_phase, "design");
        assert_eq!(manifest.workflow.limits.max_dialogue_lines, Some(24));
        assert_eq!(manifest.workflow.limits.max_generate_calls, Some(1));
        assert!(
            manifest
                .workflow
                .allowed_tools
                .iter()
                .any(|tool| tool == "mofa_slides")
        );
        assert!(manifest.terminal_output.deliver_final_artifact_only);
        assert_eq!(
            manifest.artifacts.entries.get("deck").map(String::as_str),
            Some("output/*.pptx")
        );
        assert_eq!(
            manifest
                .validation
                .on_completion
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                "file_exists:output/*.pptx",
                "file_exists:output/**/slide-*.png"
            ]
        );
    }

    #[test]
    fn site_manifest_externalizes_build_output_specific_contract() {
        let manifest = FirstPartyHarnessManifest::site_with_build_output("out");

        assert_eq!(manifest.id, "first_party.sites.out");
        assert_eq!(manifest.workspace.kind, WorkspacePolicyKind::Sites);
        assert_eq!(manifest.terminal_output.required_artifact_kind, "site");
        assert_eq!(manifest.workflow.label, "Site deliverable");
        assert_eq!(manifest.workflow.initial_phase, "scaffold");
        assert_eq!(manifest.workflow.limits.max_dialogue_lines, Some(24));
        assert_eq!(
            manifest.validation.on_completion,
            vec!["file_exists:out/index.html"]
        );
        assert_eq!(
            manifest
                .artifacts
                .entries
                .get("entrypoint")
                .map(String::as_str),
            Some("out/index.html")
        );
    }

    #[test]
    fn sites_manifest_bundles_only_generic_contract() {
        let manifest = FirstPartyHarnessManifest::sites();

        assert_eq!(manifest.id, "first_party.sites");
        assert!(manifest.validation.on_turn_end.is_empty());
        assert!(manifest.validation.on_completion.is_empty());
        assert!(manifest.artifacts.entries.is_empty());
    }
}
