//! Rules that recognise per-project build artifacts and dependency folders.
//!
//! A rule only fires when a directory has the expected name **and** a marker
//! proving which toolchain produced it (e.g. `target/` only counts next to a
//! `Cargo.toml`). This keeps diskdiet from ever touching an unrelated folder
//! that happens to be called `build` or `bin`.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum DirName {
    Exact(&'static str),
    Prefix(&'static str),
}

impl DirName {
    fn matches(self, name: &str) -> bool {
        match self {
            DirName::Exact(n) => name == n,
            DirName::Prefix(p) => name.starts_with(p),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Marker {
    /// A file (or path) next to the candidate directory, e.g. `package.json`.
    Sibling(&'static str),
    /// Any file next to the candidate directory with this extension, e.g. `tf`.
    SiblingExt(&'static str),
    /// A file inside the candidate directory, e.g. `pyvenv.cfg`.
    Inside(&'static str),
}

#[derive(Debug)]
pub struct Rule {
    pub dirs: &'static [DirName],
    /// Any one marker is enough. Empty means the directory name alone is proof.
    pub markers: &'static [Marker],
    pub ecosystem: &'static str,
    pub what: &'static str,
    pub restore: &'static str,
}

use DirName::{Exact, Prefix};
use Marker::{Inside, Sibling, SiblingExt};

const JS: &[Marker] = &[Sibling("package.json")];
const GRADLE: &[Marker] = &[
    Sibling("build.gradle"),
    Sibling("build.gradle.kts"),
    Sibling("settings.gradle"),
    Sibling("settings.gradle.kts"),
];
const PY_PROJECT: &[Marker] = &[
    Sibling("pyproject.toml"),
    Sibling("setup.py"),
    Sibling("setup.cfg"),
    Sibling("tox.ini"),
    Sibling("noxfile.py"),
];
const DOTNET: &[Marker] = &[
    SiblingExt("csproj"),
    SiblingExt("fsproj"),
    SiblingExt("vbproj"),
];

pub static RULES: &[Rule] = &[
    // ── JavaScript / TypeScript ───────────────────────────────────────────
    Rule {
        dirs: &[Exact("node_modules")],
        markers: JS,
        ecosystem: "node",
        what: "npm/yarn/pnpm dependencies",
        restore: "npm install",
    },
    Rule {
        dirs: &[Exact(".next")],
        markers: JS,
        ecosystem: "node",
        what: "Next.js build cache",
        restore: "next build",
    },
    Rule {
        dirs: &[
            Exact(".nuxt"),
            Exact(".svelte-kit"),
            Exact(".turbo"),
            Exact(".parcel-cache"),
            Exact(".angular"),
            Exact(".expo"),
            Exact(".docusaurus"),
            Exact(".nyc_output"),
            Exact(".vercel"),
            Exact(".netlify"),
            Exact(".serverless"),
        ],
        markers: JS,
        ecosystem: "node",
        what: "framework build cache",
        restore: "rebuilt on next dev/build",
    },
    // ── Rust ──────────────────────────────────────────────────────────────
    Rule {
        dirs: &[Exact("target")],
        markers: &[Sibling("Cargo.toml")],
        ecosystem: "rust",
        what: "Cargo build output",
        restore: "cargo build",
    },
    // ── JVM ───────────────────────────────────────────────────────────────
    Rule {
        dirs: &[Exact("target")],
        markers: &[Sibling("pom.xml")],
        ecosystem: "java",
        what: "Maven build output",
        restore: "mvn package",
    },
    Rule {
        dirs: &[Exact("build"), Exact(".gradle"), Exact(".kotlin")],
        markers: GRADLE,
        ecosystem: "java",
        what: "Gradle build output",
        restore: "gradle build",
    },
    Rule {
        dirs: &[Exact(".cxx"), Exact(".externalNativeBuild")],
        markers: GRADLE,
        ecosystem: "android",
        what: "Android native build cache",
        restore: "gradle build",
    },
    // ── Python ────────────────────────────────────────────────────────────
    Rule {
        dirs: &[
            Exact(".venv"),
            Exact("venv"),
            Exact("env"),
            Exact(".env"),
            Exact("virtualenv"),
            Exact(".virtualenv"),
        ],
        markers: &[Inside("pyvenv.cfg")],
        ecosystem: "python",
        what: "Python virtualenv",
        restore: "python -m venv / uv sync",
    },
    Rule {
        dirs: &[Exact(".tox"), Exact(".nox")],
        markers: PY_PROJECT,
        ecosystem: "python",
        what: "tox/nox environments",
        restore: "tox / nox",
    },
    Rule {
        dirs: &[
            Exact("__pycache__"),
            Exact(".pytest_cache"),
            Exact(".mypy_cache"),
            Exact(".ruff_cache"),
            Exact(".hypothesis"),
        ],
        markers: &[],
        ecosystem: "python",
        what: "Python bytecode/tool cache",
        restore: "regenerated automatically",
    },
    // ── Apple ─────────────────────────────────────────────────────────────
    Rule {
        dirs: &[Exact(".build")],
        markers: &[Sibling("Package.swift")],
        ecosystem: "swift",
        what: "SwiftPM build output",
        restore: "swift build",
    },
    Rule {
        dirs: &[Exact("Pods")],
        markers: &[Sibling("Podfile")],
        ecosystem: "swift",
        what: "CocoaPods dependencies",
        restore: "pod install",
    },
    // ── Dart / Flutter ────────────────────────────────────────────────────
    Rule {
        dirs: &[Exact(".dart_tool"), Exact("build")],
        markers: &[Sibling("pubspec.yaml")],
        ecosystem: "dart",
        what: "Dart/Flutter build output",
        restore: "flutter pub get / flutter build",
    },
    // ── Go / Zig / C / C++ ────────────────────────────────────────────────
    Rule {
        dirs: &[Exact("zig-cache"), Exact(".zig-cache"), Exact("zig-out")],
        markers: &[Sibling("build.zig")],
        ecosystem: "zig",
        what: "Zig build cache",
        restore: "zig build",
    },
    Rule {
        dirs: &[Prefix("cmake-build-")],
        markers: &[Sibling("CMakeLists.txt")],
        ecosystem: "c/c++",
        what: "CMake (CLion) build dir",
        restore: "cmake --build",
    },
    // ── .NET ──────────────────────────────────────────────────────────────
    Rule {
        dirs: &[Exact("bin"), Exact("obj")],
        markers: DOTNET,
        ecosystem: ".net",
        what: ".NET build output",
        restore: "dotnet build",
    },
    // ── Elixir / Haskell / Elm / PHP ──────────────────────────────────────
    Rule {
        dirs: &[Exact("_build"), Exact("deps")],
        markers: &[Sibling("mix.exs")],
        ecosystem: "elixir",
        what: "Mix build/deps",
        restore: "mix deps.get && mix compile",
    },
    Rule {
        dirs: &[Exact(".stack-work")],
        markers: &[Sibling("stack.yaml")],
        ecosystem: "haskell",
        what: "Stack build output",
        restore: "stack build",
    },
    Rule {
        dirs: &[Exact("dist-newstyle")],
        markers: &[Sibling("cabal.project"), SiblingExt("cabal")],
        ecosystem: "haskell",
        what: "Cabal build output",
        restore: "cabal build",
    },
    Rule {
        dirs: &[Exact("elm-stuff")],
        markers: &[Sibling("elm.json")],
        ecosystem: "elm",
        what: "Elm build cache",
        restore: "elm make",
    },
    Rule {
        dirs: &[Exact("vendor")],
        markers: &[Sibling("composer.json")],
        ecosystem: "php",
        what: "Composer dependencies",
        restore: "composer install",
    },
    // ── Infra ─────────────────────────────────────────────────────────────
    Rule {
        dirs: &[Exact(".terraform")],
        markers: &[SiblingExt("tf")],
        ecosystem: "terraform",
        what: "Terraform providers/modules",
        restore: "terraform init",
    },
    // ── Game engines ──────────────────────────────────────────────────────
    Rule {
        dirs: &[Exact("Library"), Exact("Temp"), Exact("Logs")],
        markers: &[Sibling("ProjectSettings/ProjectVersion.txt")],
        ecosystem: "unity",
        what: "Unity import cache",
        restore: "re-imported when the project opens",
    },
    Rule {
        dirs: &[Exact(".godot")],
        markers: &[Sibling("project.godot")],
        ecosystem: "godot",
        what: "Godot import cache",
        restore: "re-imported when the project opens",
    },
];

/// Returns the rule that claims `dir` (whose file name is `name` and which
/// lives in `parent`), if any.
pub fn match_dir(name: &str, dir: &Path, parent: &Path) -> Option<&'static Rule> {
    RULES.iter().find(|rule| {
        rule.dirs.iter().any(|d| d.matches(name))
            && (rule.markers.is_empty() || rule.markers.iter().any(|m| has_marker(*m, dir, parent)))
    })
}

/// True when `name` could be an artifact directory for *some* rule. Used to
/// ignore artifact folders when estimating how recently a project was edited.
pub fn is_artifact_name(name: &str) -> bool {
    RULES.iter().any(|r| r.dirs.iter().any(|d| d.matches(name)))
}

fn has_marker(marker: Marker, dir: &Path, parent: &Path) -> bool {
    match marker {
        Sibling(file) => parent.join(file).exists(),
        Inside(file) => dir.join(file).exists(),
        SiblingExt(ext) => fs::read_dir(parent)
            .map(|rd| {
                rd.flatten().any(|e| {
                    Path::new(&e.file_name())
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case(ext))
                })
            })
            .unwrap_or(false),
    }
}

/// Every ecosystem name a user can pass to `--only` / `--exclude`.
pub fn ecosystems() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = RULES.iter().map(|r| r.ecosystem).collect();
    v.extend(crate::caches::CACHES.iter().map(|c| c.ecosystem));
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, b"x").unwrap();
    }

    fn check(name: &str, root: &Path) -> Option<&'static str> {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        match_dir(name, &dir, root).map(|r| r.ecosystem)
    }

    #[test]
    fn target_needs_cargo_or_pom() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check("target", t.path()), None);
        touch(&t.path().join("Cargo.toml"));
        assert_eq!(check("target", t.path()), Some("rust"));

        let m = tempfile::tempdir().unwrap();
        touch(&m.path().join("pom.xml"));
        assert_eq!(check("target", m.path()), Some("java"));
    }

    #[test]
    fn plain_build_and_bin_are_left_alone() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("Makefile"));
        assert_eq!(check("build", t.path()), None);
        assert_eq!(check("bin", t.path()), None);
        assert_eq!(check("vendor", t.path()), None);
        assert_eq!(check("Library", t.path()), None);
    }

    #[test]
    fn venv_requires_pyvenv_cfg() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check(".venv", t.path()), None);
        touch(&t.path().join(".venv/pyvenv.cfg"));
        assert_eq!(check(".venv", t.path()), Some("python"));
    }

    #[test]
    fn extension_markers() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("App.CSPROJ"));
        assert_eq!(check("obj", t.path()), Some(".net"));
        touch(&t.path().join("main.tf"));
        assert_eq!(check(".terraform", t.path()), Some("terraform"));
    }

    #[test]
    fn prefix_and_nested_markers() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("CMakeLists.txt"));
        assert_eq!(check("cmake-build-debug", t.path()), Some("c/c++"));
        touch(&t.path().join("ProjectSettings/ProjectVersion.txt"));
        assert_eq!(check("Library", t.path()), Some("unity"));
    }

    #[test]
    fn node_modules() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check("node_modules", t.path()), None);
        touch(&t.path().join("package.json"));
        assert_eq!(check("node_modules", t.path()), Some("node"));
        assert!(is_artifact_name("node_modules"));
        assert!(!is_artifact_name("src"));
    }
}
