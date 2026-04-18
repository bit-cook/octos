use crate::first_party_harness::FirstPartyHarnessName;
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

const FIRST_PARTY_HARNESS_CATALOG: [FirstPartyHarnessDescriptor; 2] = [
    FirstPartyHarnessDescriptor {
        name: FirstPartyHarnessName::Slides,
        manifest_id: "first_party.slides",
        display_name: "Slides",
        summary: "Background slide-deck production with final presentation delivery.",
        workspace_kind: WorkspacePolicyKind::Slides,
        output_kind: "presentation",
        supports_build_output_override: false,
    },
    FirstPartyHarnessDescriptor {
        name: FirstPartyHarnessName::Sites,
        manifest_id: "first_party.sites",
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

pub fn resolve_first_party_harness_descriptor_by_id(
    id: &str,
) -> Option<&'static FirstPartyHarnessDescriptor> {
    first_party_harness_catalog()
        .iter()
        .find(|descriptor| descriptor.manifest_id == id)
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
        let descriptor = resolve_first_party_harness_descriptor_by_id("first_party.sites")
            .expect("sites descriptor should resolve");

        assert_eq!(descriptor.name, FirstPartyHarnessName::Sites);
        assert_eq!(descriptor.output_kind, "site");
    }
}
