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
    pub terminal_output: FirstPartyTerminalOutput,
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
