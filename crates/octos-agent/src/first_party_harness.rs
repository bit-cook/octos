use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::workspace_policy::{
    ValidationPolicy, WorkspaceArtifactsPolicy, WorkspacePolicy, WorkspacePolicyKind,
    WorkspacePolicyWorkspace, WorkspaceSnapshotTrigger, WorkspaceSpawnTaskPolicy,
    WorkspaceTrackingPolicy, WorkspaceVersionControlPolicy, WorkspaceVersionControlProvider,
};

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
        Self {
            id: "first_party.slides".into(),
            workspace: WorkspacePolicyWorkspace {
                kind: WorkspacePolicyKind::Slides,
            },
            version_control: git_turn_end_version_control(true),
            tracking: WorkspaceTrackingPolicy {
                ignore: vec![
                    "history/**".into(),
                    "output/**".into(),
                    "skill-output/**".into(),
                    "*.pptx".into(),
                    "*.tmp".into(),
                    ".DS_Store".into(),
                ],
            },
            validation: ValidationPolicy {
                on_turn_end: vec![
                    "file_exists:script.js".into(),
                    "file_exists:memory.md".into(),
                    "file_exists:changelog.md".into(),
                ],
                on_source_change: Vec::new(),
                on_completion: vec![
                    "file_exists:output/*.pptx".into(),
                    "file_exists:output/**/slide-*.png".into(),
                ],
            },
            artifacts: WorkspaceArtifactsPolicy {
                entries: BTreeMap::from([
                    ("deck".into(), "output/*.pptx".into()),
                    ("previews".into(), "output/**/slide-*.png".into()),
                ]),
            },
            spawn_tasks: BTreeMap::new(),
            workflow: FirstPartyWorkflowDeclaration {
                label: "Slides deliverable".into(),
                ack_message: "Slides generation has started in the background. Only the final deck will be delivered once the workspace contract is satisfied.".into(),
                initial_phase: "design".into(),
                allowed_tools: vec![
                    "mofa_slides".into(),
                    "read_file".into(),
                    "write_file".into(),
                    "edit_file".into(),
                    "shell".into(),
                    "glob".into(),
                    "check_background_tasks".into(),
                    "check_workspace_contract".into(),
                ],
                limits: FirstPartyWorkflowLimits {
                    max_search_passes: None,
                    max_pipeline_runs: None,
                    max_dialogue_lines: Some(24),
                    target_audio_minutes: None,
                    max_generate_calls: Some(1),
                },
                additional_instructions: "You are a background slides producer. Follow the runtime-owned phases in order: design, generate_deck, deliver_result. Write the slide script first, validate it before generation, call mofa_slides once, and deliver only the final deck artifact. Do not send intermediate previews, scratch PNGs, or alternate deck exports.".into(),
            },
            terminal_output: FirstPartyTerminalOutput {
                deliver_final_artifact_only: true,
                deliver_media_only: false,
                forbid_intermediate_files: true,
                required_artifact_kind: "presentation".into(),
            },
        }
    }

    pub fn sites() -> Self {
        Self {
            id: "first_party.sites".into(),
            workspace: WorkspacePolicyWorkspace {
                kind: WorkspacePolicyKind::Sites,
            },
            version_control: git_turn_end_version_control(true),
            tracking: WorkspaceTrackingPolicy {
                ignore: vec![
                    "node_modules/**".into(),
                    "dist/**".into(),
                    "out/**".into(),
                    "docs/**".into(),
                    "build/**".into(),
                    ".astro/**".into(),
                    ".next/**".into(),
                    ".quarto/**".into(),
                    "*.log".into(),
                    ".DS_Store".into(),
                ],
            },
            validation: ValidationPolicy::default(),
            artifacts: WorkspaceArtifactsPolicy::default(),
            spawn_tasks: BTreeMap::new(),
            workflow: FirstPartyWorkflowDeclaration {
                label: "Site deliverable".into(),
                ack_message: "Site generation has started in the background. Only the final verified site entrypoint will be delivered once the workspace contract is satisfied.".into(),
                initial_phase: "scaffold".into(),
                allowed_tools: vec![
                    "read_file".into(),
                    "write_file".into(),
                    "edit_file".into(),
                    "shell".into(),
                    "glob".into(),
                    "check_background_tasks".into(),
                    "check_workspace_contract".into(),
                ],
                limits: FirstPartyWorkflowLimits {
                    max_search_passes: None,
                    max_pipeline_runs: None,
                    max_dialogue_lines: Some(24),
                    target_audio_minutes: None,
                    max_generate_calls: Some(1),
                },
                additional_instructions: "You are a background site builder. Follow the runtime-owned phases in order: scaffold, build, deliver_result. Read the session metadata to discover the selected template and build output directory, keep edits inside the project root, and deliver only the final built site entrypoint. Do not send intermediate logs, scratch files, or alternate build artifacts.".into(),
            },
            terminal_output: FirstPartyTerminalOutput {
                deliver_final_artifact_only: true,
                deliver_media_only: false,
                forbid_intermediate_files: true,
                required_artifact_kind: "site".into(),
            },
        }
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

fn git_turn_end_version_control(fail_on_error: bool) -> WorkspaceVersionControlPolicy {
    WorkspaceVersionControlPolicy {
        provider: WorkspaceVersionControlProvider::Git,
        auto_init: true,
        trigger: WorkspaceSnapshotTrigger::TurnEnd,
        fail_on_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
