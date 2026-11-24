//! Repository tags (OpenSpec change `parent-projects-and-lenses`): each repository a Space lists
//! may have one app and one layer from the Space's own values, and a project's tags are those
//! of its tickets' repositories.
use crate::{Extra, Space};
use serde::{Deserialize, Serialize};

/// The app and layer a Space gives one of its repositories, stored in `repository_tags`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryTags {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}
impl RepositoryTags {
    pub fn get(&self, kind: TagKind) -> Option<&str> {
        match kind {
            TagKind::App => self.app.as_deref(),
            TagKind::Layer => self.layer.as_deref(),
        }
    }
    fn slot(&mut self, kind: TagKind) -> &mut Option<String> {
        match kind {
            TagKind::App => &mut self.app,
            TagKind::Layer => &mut self.layer,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.app.is_none() && self.layer.is_none() && self.extra.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagKind {
    App,
    Layer,
}
impl TagKind {
    pub const ALL: [TagKind; 2] = [Self::App, Self::Layer];
    pub fn label(self) -> &'static str {
        match self {
            Self::App => "App",
            Self::Layer => "Layer",
        }
    }
    /// What a project with tickets but none of this kind is, as the lenses name it.
    pub fn none_label(self) -> &'static str {
        match self {
            Self::App => "no app",
            Self::Layer => "no layer",
        }
    }
}

impl Space {
    /// The Space's values of `kind`, in the order it lists them.
    pub fn values(&self, kind: TagKind) -> &[String] {
        match kind {
            TagKind::App => &self.apps,
            TagKind::Layer => &self.layers,
        }
    }
    fn values_mut(&mut self, kind: TagKind) -> &mut Vec<String> {
        match kind {
            TagKind::App => &mut self.apps,
            TagKind::Layer => &mut self.layers,
        }
    }

    /// The tags this Space gives the repository named `owner/name`, ignoring case as GitHub
    /// does; none for a repository it does not list.
    pub fn tags_of(&self, identity: &str) -> Option<&RepositoryTags> {
        if !self.lists_named(identity) {
            return None;
        }
        (self.repository_tags.iter())
            .find(|(named, _)| named.eq_ignore_ascii_case(identity))
            .map(|(_, tags)| tags)
    }

    /// Gives the repository `value` as its `kind`, or none. A value the Space does not have
    /// yet joins its values, matched without case to one it has.
    pub fn set_tag(&mut self, identity: &str, kind: TagKind, value: Option<&str>) {
        let value = value.map(str::trim).filter(|v| !v.is_empty()).map(|value| {
            let values = self.values_mut(kind);
            match values.iter().find(|v| v.eq_ignore_ascii_case(value)) {
                Some(known) => known.clone(),
                None => {
                    values.push(value.to_owned());
                    value.to_owned()
                }
            }
        });
        let key = (self.repository_tags.keys())
            .find(|named| named.eq_ignore_ascii_case(identity))
            .cloned()
            .unwrap_or_else(|| identity.to_owned());
        let mut tags = self.repository_tags.remove(&key).unwrap_or_default();
        *tags.slot(kind) = value;
        if !tags.is_empty() {
            self.repository_tags.insert(key, tags);
        }
    }

    /// The repositories this Space lists that have `value` as their `kind`.
    pub fn using(&self, kind: TagKind, value: &str) -> Vec<String> {
        (self.repositories.iter())
            .filter(|identity| {
                self.tags_of(identity)
                    .is_some_and(|tags| tags.get(kind) == Some(value))
            })
            .cloned()
            .collect()
    }

    /// Removes `value` from the Space's values of `kind`, and from every repository that has it.
    pub fn remove_value(&mut self, kind: TagKind, value: &str) {
        self.values_mut(kind).retain(|v| v != value);
        for tags in self.repository_tags.values_mut() {
            if tags.get(kind) == Some(value) {
                *tags.slot(kind) = None;
            }
        }
        self.repository_tags.retain(|_, tags| !tags.is_empty());
    }

    /// What adding the repository `identity` suggests: the tags of the listed repository whose
    /// name shares the longest leading run of hyphen-separated words with it, the alphabetically
    /// first on a tie. None when no name shares its first word, or that repository is untagged.
    pub fn suggested_tags(&self, identity: &str) -> Option<RepositoryTags> {
        let words = |identity: &str| -> Vec<String> {
            let name = identity.rsplit('/').next().unwrap_or(identity);
            name.to_ascii_lowercase()
                .split('-')
                .map(str::to_owned)
                .collect()
        };
        let added = words(identity);
        let shared = |other: &str| {
            (added.iter().zip(words(other)))
                .take_while(|(a, b)| *a == b)
                .count()
        };
        let closest = (self.repositories.iter())
            .filter(|other| !other.eq_ignore_ascii_case(identity))
            .map(|other| (shared(other), words(other), other))
            .filter(|(count, _, _)| *count > 0)
            .min_by(|(a, a_name, a_id), (b, b_name, b_id)| {
                (b.cmp(a))
                    .then_with(|| a_name.cmp(b_name))
                    .then_with(|| a_id.cmp(b_id))
            })?;
        let tags = self.tags_of(closest.2)?;
        let suggested = RepositoryTags {
            app: tags.app.clone(),
            layer: tags.layer.clone(),
            extra: Extra::new(),
        };
        (!suggested.is_empty()).then_some(suggested)
    }
}

/// A project's apps and layers: those its Space gives the repositories of all its tickets, in
/// any state, in the Space's order. Derived by the host, never set by hand.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTags {
    /// It has a ticket. Without one it is "not started" and has no tags.
    #[serde(default)]
    pub started: bool,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub layers: Vec<String>,
}
impl ProjectTags {
    /// The tags of a project in `space` whose tickets work in `repositories`, each its GitHub
    /// `owner/name`, or none for one without.
    pub fn derive<'a>(
        space: Option<&Space>,
        repositories: impl IntoIterator<Item = Option<&'a str>>,
    ) -> Self {
        let repositories: Vec<Option<&str>> = repositories.into_iter().collect();
        let tagged: Vec<&RepositoryTags> = (repositories.iter().flatten())
            .filter_map(|identity| space?.tags_of(identity))
            .collect();
        let used = |kind: TagKind| -> Vec<String> {
            (space.map_or(&[][..], |space| space.values(kind)).iter())
                .filter(|value| tagged.iter().any(|tags| tags.get(kind) == Some(value)))
                .cloned()
                .collect()
        };
        Self {
            started: !repositories.is_empty(),
            apps: used(TagKind::App),
            layers: used(TagKind::Layer),
        }
    }
    pub fn of(&self, kind: TagKind) -> &[String] {
        match kind {
            TagKind::App => &self.apps,
            TagKind::Layer => &self.layers,
        }
    }
    /// It has tickets but none of `kind`: "no app" or "no layer".
    pub fn has_none(&self, kind: TagKind) -> bool {
        self.started && self.of(kind).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SpaceStorage, StorageRepository};

    fn celadon(repositories: &[&str]) -> Space {
        Space {
            id: crate::SpaceId("celadon".into()),
            name: "Celadon".into(),
            color: None,
            description: String::new(),
            additions: String::new(),
            behaviors: Default::default(),
            repository_settings: Default::default(),
            brain: None,
            storage: SpaceStorage {
                repository: StorageRepository::Local { id: "x".into() },
                folder: "celadon".into(),
                root: String::new(),
            },
            repositories: repositories
                .iter()
                .map(|r| format!("celadon-theme/{r}"))
                .collect(),
            apps: vec![],
            layers: vec![],
            repository_tags: Default::default(),
            models: Default::default(),
            reviews: Default::default(),
            sources: vec![],
            extra: Default::default(),
        }
    }

    #[test]
    fn a_suggestion_copies_the_longest_shared_run_and_the_first_name_on_a_tie() {
        let mut space = celadon(&["palette-api", "palette-build", "palette-api-docs", "nvim"]);
        space.set_tag("celadon-theme/palette-api", TagKind::Layer, Some("backend"));
        space.set_tag("celadon-theme/palette-build", TagKind::Layer, Some("infra"));
        space.set_tag("celadon-theme/palette-api-docs", TagKind::App, Some("docs"));

        let suggested = space.suggested_tags("celadon-theme/palette-cli").unwrap();
        assert_eq!(
            (suggested.app, suggested.layer.as_deref()),
            (None, Some("backend"))
        );

        let longer = space
            .suggested_tags("celadon-theme/palette-api-docs-site")
            .unwrap();
        assert_eq!(longer.app.as_deref(), Some("docs"));
        // Two words with both palette-api and palette-api-docs: the first name wins.
        let tied = space.suggested_tags("celadon-theme/palette-api-tests");
        assert_eq!(tied.unwrap().layer.as_deref(), Some("backend"));

        assert_eq!(space.suggested_tags("celadon-theme/wallpapers"), None);
        // "nvim" is untagged, so the only match suggests nothing.
        assert_eq!(space.suggested_tags("Other/NVIM-lua"), None);
    }

    #[test]
    fn typed_values_join_the_space_and_removing_one_clears_it() {
        let mut space = celadon(&["website", "palette-api", "nvim"]);
        space.set_tag("celadon-theme/website", TagKind::App, Some(" web "));
        space.set_tag("celadon-theme/nvim", TagKind::App, Some("WEB"));
        space.set_tag("celadon-theme/palette-api", TagKind::Layer, Some("backend"));
        space.set_tag("celadon-theme/website", TagKind::Layer, Some("backend"));
        assert_eq!(space.apps, ["web"]);
        assert_eq!(space.using(TagKind::App, "web").len(), 2);

        space.remove_value(TagKind::Layer, "backend");
        assert!(space.layers.is_empty());
        assert!(space.using(TagKind::Layer, "backend").is_empty());
        assert_eq!(space.repository_tags.len(), 2);

        space.set_tag("celadon-theme/nvim", TagKind::App, None);
        assert!(space.tags_of("celadon-theme/nvim").is_none());
    }

    #[test]
    fn a_project_takes_the_union_of_its_tickets_repositories() {
        let mut space = celadon(&["nvim", "palette-api", "celadon-infra", "loose"]);
        space.set_tag("celadon-theme/nvim", TagKind::App, Some("nvim"));
        space.set_tag("celadon-theme/nvim", TagKind::Layer, Some("frontend"));
        space.set_tag("celadon-theme/palette-api", TagKind::Layer, Some("backend"));
        space.set_tag("celadon-theme/celadon-infra", TagKind::Layer, Some("infra"));

        let contrast = ProjectTags::derive(
            Some(&space),
            [
                Some("celadon-theme/palette-api"),
                Some("Celadon-Theme/NVIM"),
            ],
        );
        assert_eq!(contrast.apps, ["nvim"]);
        assert_eq!(contrast.layers, ["frontend", "backend"]);

        let dns = ProjectTags::derive(Some(&space), [Some("celadon-theme/celadon-infra")]);
        assert!(dns.has_none(TagKind::App));
        assert_eq!(dns.layers, ["infra"]);

        let local = ProjectTags::derive(Some(&space), [None, Some("celadon-theme/loose")]);
        assert!(local.has_none(TagKind::App) && local.has_none(TagKind::Layer));

        let none = ProjectTags::derive(Some(&space), []);
        assert!(!none.started && !none.has_none(TagKind::App));
    }

    #[test]
    fn a_repository_the_space_does_not_list_is_untagged() {
        let mut space = celadon(&["website"]);
        space.set_tag("celadon-theme/website", TagKind::Layer, Some("frontend"));
        space.repositories.clear();
        let tags = ProjectTags::derive(Some(&space), [Some("celadon-theme/website")]);
        assert!(tags.has_none(TagKind::Layer));
    }
}
