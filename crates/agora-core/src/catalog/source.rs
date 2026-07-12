use crate::ctx::Ctx;
use crate::error::LauncherError;
use std::path::Path;

/// How a project is referenced across sources. The join key for enrichment.
/// `Modrinth("<project_id>")` is the canonical key when available; `Agora("<slug>")`
/// is the synthetic fallback for GitHub-only curated mods (plan Phase 2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "source", content = "id")]
pub enum ProjectRef {
    Modrinth(String),
    Agora(String),
    GithubRelease(String),
}

/// A unified catalog item returned by any source's search.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CatalogItem {
    pub project_ref: ProjectRef,
    pub name: String,
    pub slug: String,
    pub author: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub follows: Option<u64>,
    pub categories: Vec<String>,
    pub loader: Option<String>,
    pub game_versions: Vec<String>,
}

/// A version of a project downloadable from a source.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Version {
    pub project_ref: ProjectRef,
    pub version_number: String,
    pub name: String,
    pub filename: String,
    pub download_url: String,
    pub hashes: Hashes,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub release_type: ReleaseType,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hashes {
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReleaseType {
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Dependency {
    pub project_ref: ProjectRef,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

/// Search query passed to `CatalogSource::search`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchQuery {
    pub query: Option<String>,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub categories: Vec<String>,
    pub limit: u32,
    pub offset: u32,
}

/// A computed dependency graph (transitive resolution result).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DepGraph {
    pub nodes: Vec<Version>,
    pub edges: Vec<(usize, usize)>, // index pairs into nodes
}

/// The source-agnostic catalog abstraction (adapted from Prism's ResourceAPI).
#[async_trait::async_trait]
pub trait CatalogSource: Send + Sync {
    /// Human-readable source name (e.g. "Modrinth").
    fn name(&self) -> &str;

    /// Whether this source is enabled by the user's settings.
    fn is_enabled(&self, ctx: &Ctx) -> bool;

    /// Search this source's catalog.
    async fn search(&self, ctx: &Ctx, q: &SearchQuery) -> Result<Vec<CatalogItem>, LauncherError>;

    /// Fetch a single project by its canonical reference.
    async fn project(&self, ctx: &Ctx, id: &ProjectRef) -> Result<CatalogItem, LauncherError>;

    /// List all versions for a project.
    async fn versions(&self, ctx: &Ctx, id: &ProjectRef) -> Result<Vec<Version>, LauncherError>;

    /// Resolve transitive dependencies for a version.
    async fn resolve_dependencies(&self, ctx: &Ctx, v: &Version)
        -> Result<DepGraph, LauncherError>;

    /// Download a version file to `dest`, returning verified hashes.
    async fn download(&self, ctx: &Ctx, v: &Version, dest: &Path) -> Result<Hashes, LauncherError>;

    /// Verify a local file against expected hashes.
    async fn verify(&self, file: &Path, expected: &Hashes) -> Result<(), LauncherError>;
}

// ---------------------------------------------------------------------------
// Tests: ProjectRef and Hashes serde round-trips
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_ref_modrinth_roundtrip() {
        let original = ProjectRef::Modrinth("a1b2c3d4".to_string());
        let json = serde_json::to_string(&original).unwrap();
        let restored: ProjectRef = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
        assert!(json.contains("\"source\":\"Modrinth\""));
        assert!(json.contains("\"id\":\"a1b2c3d4\""));
    }

    #[test]
    fn project_ref_agora_roundtrip() {
        let original = ProjectRef::Agora("my-mod".to_string());
        let json = serde_json::to_string(&original).unwrap();
        let restored: ProjectRef = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
        assert!(json.contains("\"source\":\"Agora\""));
        assert!(json.contains("\"id\":\"my-mod\""));
    }

    #[test]
    fn project_ref_github_release_roundtrip() {
        let original = ProjectRef::GithubRelease("owner/repo".to_string());
        let json = serde_json::to_string(&original).unwrap();
        let restored: ProjectRef = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
        assert!(json.contains("\"source\":\"GithubRelease\""));
        assert!(json.contains("\"id\":\"owner/repo\""));
    }

    #[test]
    fn project_ref_discriminants_match() {
        assert_ne!(
            ProjectRef::Modrinth("x".to_string()),
            ProjectRef::Modrinth("y".to_string())
        );
        assert_ne!(
            ProjectRef::Modrinth("x".to_string()),
            ProjectRef::Agora("x".to_string())
        );
        assert_ne!(
            ProjectRef::Modrinth("x".to_string()),
            ProjectRef::GithubRelease("x".to_string())
        );
    }

    #[test]
    fn hashes_default_serialization() {
        let h = Hashes::default();
        let json = serde_json::to_string(&h).unwrap();
        // All fields should be null (missing from default).
        assert_eq!(json, r#"{"sha1":null,"sha512":null,"sha256":null}"#);
    }

    #[test]
    fn hashes_with_values_roundtrip() {
        let h = Hashes {
            sha1: Some("abc123".to_string()),
            sha512: Some("def456".to_string()),
            sha256: Some("ghi789".to_string()),
        };
        let json = serde_json::to_string(&h).unwrap();
        let restored: Hashes = serde_json::from_str(&json).unwrap();
        assert_eq!(h.sha1, restored.sha1);
        assert_eq!(h.sha512, restored.sha512);
        assert_eq!(h.sha256, restored.sha256);
    }

    #[test]
    fn release_type_roundtrip() {
        for (variant, tag) in [
            (ReleaseType::Release, "Release"),
            (ReleaseType::Beta, "Beta"),
            (ReleaseType::Alpha, "Alpha"),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert!(json.contains(tag));
            let restored: ReleaseType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, restored);
        }
    }

    #[test]
    fn dependency_kind_roundtrip() {
        for kind in [
            DependencyKind::Required,
            DependencyKind::Optional,
            DependencyKind::Incompatible,
            DependencyKind::Embedded,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let restored: DependencyKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, restored);
        }
    }

    #[test]
    fn catalog_item_roundtrip() {
        let item = CatalogItem {
            project_ref: ProjectRef::Modrinth("test-id".to_string()),
            name: "Test Mod".to_string(),
            slug: "test-mod".to_string(),
            author: "TestAuthor".to_string(),
            description: "A test mod".to_string(),
            icon_url: Some("https://example.com/icon.png".to_string()),
            downloads: 12345,
            follows: Some(678),
            categories: vec!["Magic".to_string(), "Adventure".to_string()],
            loader: Some("fabric".to_string()),
            game_versions: vec!["1.21.1".to_string()],
        };
        let json = serde_json::to_string(&item).unwrap();
        let restored: CatalogItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item.project_ref, restored.project_ref);
        assert_eq!(item.name, restored.name);
        assert_eq!(item.slug, restored.slug);
        assert_eq!(item.author, restored.author);
        assert_eq!(item.description, restored.description);
        assert_eq!(item.icon_url, restored.icon_url);
        assert_eq!(item.downloads, restored.downloads);
        assert_eq!(item.follows, restored.follows);
        assert_eq!(item.categories, restored.categories);
        assert_eq!(item.loader, restored.loader);
        assert_eq!(item.game_versions, restored.game_versions);
    }

    #[test]
    fn search_query_default_roundtrip() {
        let q = SearchQuery::default();
        let json = serde_json::to_string(&q).unwrap();
        let restored: SearchQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(q.query, restored.query);
        assert_eq!(q.game_version, restored.game_version);
        assert_eq!(q.loader, restored.loader);
        assert_eq!(q.categories, restored.categories);
        assert_eq!(q.limit, restored.limit);
        assert_eq!(q.offset, restored.offset);
    }

    #[test]
    fn dep_graph_roundtrip() {
        let graph = DepGraph {
            nodes: vec![],
            edges: vec![(0, 1), (1, 2)],
        };
        let json = serde_json::to_string(&graph).unwrap();
        let restored: DepGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(graph.nodes, restored.nodes);
        assert_eq!(graph.edges, restored.edges);
    }
}
