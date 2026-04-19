use crate::first_party_harness::{
    FIRST_PARTY_SITES_HARNESS_ID, FIRST_PARTY_SLIDES_HARNESS_ID, FirstPartyHarnessManifest,
    FirstPartyHarnessName, resolve_first_party_harness_by_id,
};
use crate::workspace_policy::WorkspacePolicyKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstPartyHarnessDescriptor {
    pub name: FirstPartyHarnessName,
    pub manifest_id: &'static str,
    pub display_name: &'static str,
    pub summary: &'static str,
    pub workspace_kind: WorkspacePolicyKind,
    pub output_kind: &'static str,
    pub supports_build_output_override: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedFirstPartyHarness {
    pub descriptor: &'static FirstPartyHarnessDescriptor,
    pub manifest: FirstPartyHarnessManifest,
}

const FIRST_PARTY_HARNESS_CATALOG: [FirstPartyHarnessDescriptor; 2] = [
    FirstPartyHarnessDescriptor {
        name: FirstPartyHarnessName::Slides,
        manifest_id: FIRST_PARTY_SLIDES_HARNESS_ID,
        display_name: "Slides",
        summary: "Background slide-deck production with final presentation delivery.",
        workspace_kind: WorkspacePolicyKind::Slides,
        output_kind: "presentation",
        supports_build_output_override: false,
    },
    FirstPartyHarnessDescriptor {
        name: FirstPartyHarnessName::Sites,
        manifest_id: FIRST_PARTY_SITES_HARNESS_ID,
        display_name: "Sites",
        summary: "Background site builds with final verified entrypoint delivery.",
        workspace_kind: WorkspacePolicyKind::Sites,
        output_kind: "site",
        supports_build_output_override: true,
    },
];

pub fn first_party_harness_catalog() -> &'static [FirstPartyHarnessDescriptor] {
    &FIRST_PARTY_HARNESS_CATALOG
}

pub fn first_party_harness_descriptor(
    name: FirstPartyHarnessName,
) -> &'static FirstPartyHarnessDescriptor {
    first_party_harness_catalog()
        .iter()
        .find(|descriptor| descriptor.name == name)
        .unwrap_or_else(|| {
            panic!(
                "missing first-party harness descriptor for {}",
                name.as_str()
            )
        })
}

pub fn resolve_first_party_harness(name: FirstPartyHarnessName) -> ResolvedFirstPartyHarness {
    let descriptor = first_party_harness_descriptor(name);
    let manifest = resolve_first_party_harness_by_id(descriptor.manifest_id).unwrap_or_else(|| {
        panic!(
            "missing first-party harness manifest {}",
            descriptor.manifest_id
        )
    });

    ResolvedFirstPartyHarness {
        descriptor,
        manifest,
    }
}

pub fn resolve_first_party_harness_by_manifest_id(id: &str) -> Option<ResolvedFirstPartyHarness> {
    resolve_first_party_harness_descriptor_by_id(id)
        .map(|descriptor| resolve_first_party_harness(descriptor.name))
}

pub fn resolve_first_party_harness_descriptor_by_id(
    id: &str,
) -> Option<&'static FirstPartyHarnessDescriptor> {
    first_party_harness_catalog()
        .iter()
        .find(|descriptor| descriptor.manifest_id == id)
}

pub fn resolve_first_party_harness_for_workspace_kind(
    workspace_kind: WorkspacePolicyKind,
) -> Option<ResolvedFirstPartyHarness> {
    first_party_harness_catalog()
        .iter()
        .find(|descriptor| descriptor.workspace_kind == workspace_kind)
        .map(|descriptor| resolve_first_party_harness(descriptor.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_party_harness::first_party_harness_entry;

    #[test]
    fn catalog_lists_expected_first_party_descriptors() {
        let catalog = first_party_harness_catalog();

        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].name, FirstPartyHarnessName::Slides);
        assert_eq!(catalog[0].display_name, "Slides");
        assert_eq!(catalog[1].name, FirstPartyHarnessName::Sites);
        assert!(catalog[1].supports_build_output_override);
    }

    #[test]
    fn catalog_descriptor_matches_registry_entry() {
        let descriptor = first_party_harness_descriptor(FirstPartyHarnessName::Slides);
        let registry_entry = first_party_harness_entry(descriptor.name);

        assert_eq!(descriptor.manifest_id, registry_entry.manifest_id);
    }

    #[test]
    fn catalog_resolves_descriptor_by_manifest_id() {
        let descriptor = resolve_first_party_harness_descriptor_by_id(FIRST_PARTY_SITES_HARNESS_ID)
            .expect("sites descriptor should resolve");

        assert_eq!(descriptor.name, FirstPartyHarnessName::Sites);
        assert_eq!(descriptor.output_kind, "site");
    }

    #[test]
    fn catalog_resolves_manifest_and_descriptor_together() {
        let resolved = resolve_first_party_harness(FirstPartyHarnessName::Slides);

        assert_eq!(resolved.descriptor.manifest_id, resolved.manifest.id);
        assert_eq!(resolved.descriptor.output_kind, "presentation");
    }

    #[test]
    fn catalog_resolves_by_workspace_kind() {
        let resolved = resolve_first_party_harness_for_workspace_kind(WorkspacePolicyKind::Sites)
            .expect("sites harness should resolve");

        assert_eq!(resolved.descriptor.name, FirstPartyHarnessName::Sites);
        assert_eq!(
            resolved.manifest.terminal_output.required_artifact_kind,
            "site"
        );
    }

    #[test]
    fn catalog_resolves_by_manifest_id() {
        let resolved = resolve_first_party_harness_by_manifest_id(FIRST_PARTY_SLIDES_HARNESS_ID)
            .expect("slides harness should resolve");

        assert_eq!(resolved.descriptor.name, FirstPartyHarnessName::Slides);
        assert_eq!(resolved.manifest.id, FIRST_PARTY_SLIDES_HARNESS_ID);
    }
}
