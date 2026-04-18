use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::workspace_policy::{
    ValidationPolicy, WorkspaceArtifactsPolicy, WorkspacePolicy, WorkspacePolicyWorkspace,
    WorkspaceSpawnTaskPolicy, WorkspaceTrackingPolicy, WorkspaceVersionControlPolicy,
};

const SLIDES_MANIFEST_TOML: &str = include_str!("first_party_harness/slides.toml");
const SITES_MANIFEST_TOML: &str = include_str!("first_party_harness/sites.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FirstPartyHarnessName {
    Slides,
    Sites,
}

impl FirstPartyHarnessName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Slides => "slides",
            Self::Sites => "sites",
        }
    }

    pub fn descriptor(
        self,
    ) -> &'static crate::first_party_harness_catalog::FirstPartyHarnessDescriptor {
        crate::first_party_harness_catalog::first_party_harness_descriptor(self)
    }

    pub fn manifest(self) -> FirstPartyHarnessManifest {
        first_party_harness_entry(self).load()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstPartyHarnessRegistryEntry {
    pub name: FirstPartyHarnessName,
    pub manifest_id: &'static str,
    pub manifest_asset: &'static str,
}

impl FirstPartyHarnessRegistryEntry {
    pub fn load(self) -> FirstPartyHarnessManifest {
        bundled_manifest(self.manifest_asset, manifest_source(self.name))
    }
}

const FIRST_PARTY_HARNESS_REGISTRY: [FirstPartyHarnessRegistryEntry; 2] = [
    FirstPartyHarnessRegistryEntry {
        name: FirstPartyHarnessName::Slides,
        manifest_id: "first_party.slides",
        manifest_asset: "slides.toml",
    },
    FirstPartyHarnessRegistryEntry {
        name: FirstPartyHarnessName::Sites,
        manifest_id: "first_party.sites",
        manifest_asset: "sites.toml",
    },
];

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
        FirstPartyHarnessName::Slides.manifest()
    }

    pub fn sites() -> Self {
        FirstPartyHarnessName::Sites.manifest()
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

pub fn first_party_harness_registry() -> &'static [FirstPartyHarnessRegistryEntry] {
    &FIRST_PARTY_HARNESS_REGISTRY
}

pub fn first_party_harness_entry(
    name: FirstPartyHarnessName,
) -> &'static FirstPartyHarnessRegistryEntry {
    first_party_harness_registry()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| {
            panic!(
                "missing first-party harness registry entry for {}",
                name.as_str()
            )
        })
}

pub fn resolve_first_party_harness_by_id(id: &str) -> Option<FirstPartyHarnessManifest> {
    first_party_harness_registry()
        .iter()
        .find(|entry| entry.manifest_id == id)
        .map(|entry| entry.load())
}

fn manifest_source(name: FirstPartyHarnessName) -> &'static str {
    match name {
        FirstPartyHarnessName::Slides => SLIDES_MANIFEST_TOML,
        FirstPartyHarnessName::Sites => SITES_MANIFEST_TOML,
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
    fn registry_lists_bundled_first_party_manifests() {
        let registry = first_party_harness_registry();

        assert_eq!(registry.len(), 2);
        assert_eq!(registry[0].name, FirstPartyHarnessName::Slides);
        assert_eq!(registry[0].manifest_id, "first_party.slides");
        assert_eq!(registry[1].name, FirstPartyHarnessName::Sites);
        assert_eq!(registry[1].manifest_id, "first_party.sites");
    }

    #[test]
    fn registry_resolves_manifest_by_id() {
        let manifest = resolve_first_party_harness_by_id("first_party.slides")
            .expect("slides manifest should resolve");

        assert_eq!(manifest.id, "first_party.slides");
        assert_eq!(manifest.workflow.label, "Slides deliverable");
    }

    #[test]
    fn harness_name_exposes_descriptor_view() {
        let descriptor = FirstPartyHarnessName::Slides.descriptor();

        assert_eq!(descriptor.manifest_id, "first_party.slides");
        assert_eq!(descriptor.output_kind, "presentation");
    }

    #[test]
    fn slides_manifest_declares_expected_contract() {
        let manifest = FirstPartyHarnessName::Slides.manifest();

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
        let manifest = FirstPartyHarnessName::Sites.manifest();

        assert_eq!(manifest.id, "first_party.sites");
        assert!(manifest.validation.on_turn_end.is_empty());
        assert!(manifest.validation.on_completion.is_empty());
        assert!(manifest.artifacts.entries.is_empty());
    }
}
