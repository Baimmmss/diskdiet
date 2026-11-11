//! Machine-wide caches kept by package managers and IDEs. They live at
//! well-known paths, so instead of searching for them we just check whether
//! each one exists.

use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum Base {
    Home,
    /// An environment variable such as `LOCALAPPDATA` or `CARGO_HOME`.
    Env(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Any,
    Mac,
    Linux,
    Windows,
}

impl Os {
    fn current(self) -> bool {
        match self {
            Os::Any => true,
            Os::Mac => cfg!(target_os = "macos"),
            Os::Linux => cfg!(all(unix, not(target_os = "macos"))),
            Os::Windows => cfg!(windows),
        }
    }
}

#[derive(Debug)]
pub struct Cache {
    pub ecosystem: &'static str,
    pub what: &'static str,
    pub restore: &'static str,
    pub os: Os,
    pub base: Base,
    pub rel: &'static str,
}

const fn c(
    ecosystem: &'static str,
    what: &'static str,
    restore: &'static str,
    os: Os,
    base: Base,
    rel: &'static str,
) -> Cache {
    Cache {
        ecosystem,
        what,
        restore,
        os,
        base,
        rel,
    }
}

use Base::{Env, Home};
use Os::{Any, Linux, Mac, Windows};

const REDOWNLOAD: &str = "re-downloaded on demand";

pub static CACHES: &[Cache] = &[
    // Apple
    c(
        "xcode",
        "Xcode DerivedData",
        "rebuilt on next Xcode build",
        Mac,
        Home,
        "Library/Developer/Xcode/DerivedData",
    ),
    c(
        "xcode",
        "iOS DeviceSupport symbols",
        "re-copied when a device connects",
        Mac,
        Home,
        "Library/Developer/Xcode/iOS DeviceSupport",
    ),
    c(
        "xcode",
        "Simulator caches",
        "regenerated automatically",
        Mac,
        Home,
        "Library/Developer/CoreSimulator/Caches",
    ),
    c(
        "swift",
        "CocoaPods cache",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/CocoaPods",
    ),
    c(
        "homebrew",
        "Homebrew downloads",
        "brew re-downloads as needed",
        Mac,
        Home,
        "Library/Caches/Homebrew",
    ),
    c(
        "homebrew",
        "Homebrew downloads",
        "brew re-downloads as needed",
        Linux,
        Home,
        ".cache/Homebrew",
    ),
    // JavaScript
    c("node", "npm cache", REDOWNLOAD, Any, Home, ".npm/_cacache"),
    c(
        "node",
        "npm cache",
        REDOWNLOAD,
        Windows,
        Env("LOCALAPPDATA"),
        "npm-cache",
    ),
    c(
        "node",
        "Yarn v1 cache",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/Yarn",
    ),
    c(
        "node",
        "Yarn v1 cache",
        REDOWNLOAD,
        Linux,
        Home,
        ".cache/yarn",
    ),
    c(
        "node",
        "Yarn v1 cache",
        REDOWNLOAD,
        Windows,
        Env("LOCALAPPDATA"),
        "Yarn/Cache",
    ),
    c(
        "node",
        "Yarn Berry cache",
        REDOWNLOAD,
        Any,
        Home,
        ".yarn/berry/cache",
    ),
    c(
        "node",
        "pnpm store",
        "run `pnpm install` again in projects",
        Mac,
        Home,
        "Library/pnpm/store",
    ),
    c(
        "node",
        "pnpm store",
        "run `pnpm install` again in projects",
        Linux,
        Home,
        ".local/share/pnpm/store",
    ),
    c(
        "node",
        "pnpm store",
        "run `pnpm install` again in projects",
        Windows,
        Env("LOCALAPPDATA"),
        "pnpm/store",
    ),
    c(
        "node",
        "Bun install cache",
        REDOWNLOAD,
        Any,
        Home,
        ".bun/install/cache",
    ),
    c(
        "node",
        "node-gyp headers",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/node-gyp",
    ),
    c(
        "node",
        "node-gyp headers",
        REDOWNLOAD,
        Linux,
        Home,
        ".cache/node-gyp",
    ),
    c(
        "node",
        "Electron downloads",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/electron",
    ),
    c(
        "node",
        "Electron downloads",
        REDOWNLOAD,
        Linux,
        Home,
        ".cache/electron",
    ),
    c(
        "node",
        "Playwright browsers",
        "npx playwright install",
        Mac,
        Home,
        "Library/Caches/ms-playwright",
    ),
    c(
        "node",
        "Playwright browsers",
        "npx playwright install",
        Linux,
        Home,
        ".cache/ms-playwright",
    ),
    c(
        "node",
        "Playwright browsers",
        "npx playwright install",
        Windows,
        Env("LOCALAPPDATA"),
        "ms-playwright",
    ),
    c(
        "node",
        "Puppeteer browsers",
        REDOWNLOAD,
        Any,
        Home,
        ".cache/puppeteer",
    ),
    c(
        "deno",
        "Deno cache",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/deno",
    ),
    c("deno", "Deno cache", REDOWNLOAD, Linux, Home, ".cache/deno"),
    // Python
    c(
        "python",
        "pip cache",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/pip",
    ),
    c("python", "pip cache", REDOWNLOAD, Linux, Home, ".cache/pip"),
    c(
        "python",
        "pip cache",
        REDOWNLOAD,
        Windows,
        Env("LOCALAPPDATA"),
        "pip/Cache",
    ),
    c("python", "uv cache", REDOWNLOAD, Any, Home, ".cache/uv"),
    c(
        "python",
        "uv cache",
        REDOWNLOAD,
        Windows,
        Env("LOCALAPPDATA"),
        "uv/cache",
    ),
    c(
        "python",
        "Poetry cache",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/pypoetry",
    ),
    c(
        "python",
        "Poetry cache",
        REDOWNLOAD,
        Linux,
        Home,
        ".cache/pypoetry",
    ),
    c(
        "python",
        "Hugging Face models",
        "re-downloaded on demand (can be slow)",
        Any,
        Home,
        ".cache/huggingface/hub",
    ),
    // Rust
    c(
        "rust",
        "Cargo registry cache",
        REDOWNLOAD,
        Any,
        Env("CARGO_HOME"),
        "registry/cache",
    ),
    c(
        "rust",
        "Cargo registry sources",
        "re-extracted on demand",
        Any,
        Env("CARGO_HOME"),
        "registry/src",
    ),
    c(
        "rust",
        "Cargo git checkouts",
        REDOWNLOAD,
        Any,
        Env("CARGO_HOME"),
        "git/checkouts",
    ),
    c(
        "rust",
        "Cargo registry cache",
        REDOWNLOAD,
        Any,
        Home,
        ".cargo/registry/cache",
    ),
    c(
        "rust",
        "Cargo registry sources",
        "re-extracted on demand",
        Any,
        Home,
        ".cargo/registry/src",
    ),
    c(
        "rust",
        "Cargo git checkouts",
        REDOWNLOAD,
        Any,
        Home,
        ".cargo/git/checkouts",
    ),
    // Go
    c(
        "go",
        "Go build cache",
        "rebuilt on next go build",
        Mac,
        Home,
        "Library/Caches/go-build",
    ),
    c(
        "go",
        "Go build cache",
        "rebuilt on next go build",
        Linux,
        Home,
        ".cache/go-build",
    ),
    c(
        "go",
        "Go build cache",
        "rebuilt on next go build",
        Windows,
        Env("LOCALAPPDATA"),
        "go-build",
    ),
    c(
        "go",
        "Go module cache",
        REDOWNLOAD,
        Any,
        Env("GOPATH"),
        "pkg/mod",
    ),
    c("go", "Go module cache", REDOWNLOAD, Any, Home, "go/pkg/mod"),
    // JVM / .NET
    c(
        "java",
        "Gradle caches",
        REDOWNLOAD,
        Any,
        Home,
        ".gradle/caches",
    ),
    c(
        "java",
        "Gradle wrapper dists",
        REDOWNLOAD,
        Any,
        Home,
        ".gradle/wrapper/dists",
    ),
    c(
        "java",
        "Maven repository",
        REDOWNLOAD,
        Any,
        Home,
        ".m2/repository",
    ),
    c(
        ".net",
        "NuGet packages",
        REDOWNLOAD,
        Any,
        Home,
        ".nuget/packages",
    ),
    // Dart / Flutter
    c(
        "dart",
        "Dart pub cache",
        "flutter pub get / dart pub get",
        Any,
        Home,
        ".pub-cache/hosted",
    ),
    c(
        "dart",
        "Dart pub cache",
        "flutter pub get / dart pub get",
        Windows,
        Env("LOCALAPPDATA"),
        "Pub/Cache/hosted",
    ),
    // PHP
    c(
        "php",
        "Composer cache",
        REDOWNLOAD,
        Mac,
        Home,
        "Library/Caches/composer",
    ),
    c(
        "php",
        "Composer cache",
        REDOWNLOAD,
        Linux,
        Home,
        ".cache/composer",
    ),
];

pub fn home_dir() -> Option<PathBuf> {
    #[allow(deprecated)] // fixed on all supported platforms since Rust 1.85
    env::home_dir().filter(|p| !p.as_os_str().is_empty())
}

/// Every cache location that applies to this OS, whether or not it exists.
/// Paths are de-duplicated (e.g. `$CARGO_HOME` usually *is* `~/.cargo`).
pub fn resolve() -> Vec<(&'static Cache, PathBuf)> {
    let home = home_dir();
    let mut out: Vec<(&'static Cache, PathBuf)> = Vec::new();
    for cache in CACHES.iter().filter(|c| c.os.current()) {
        let base = match cache.base {
            Home => home.clone(),
            Env(var) => env::var_os(var)
                .filter(|v| !v.is_empty())
                // GOPATH may be a list; the module cache lives in the first entry.
                .and_then(|v| env::split_paths(&v).next()),
        };
        let Some(base) = base else { continue };
        let path = base.join(cache.rel);
        let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
        if out.iter().any(|(_, p)| p == &canon) {
            continue;
        }
        out.push((cache, canon));
    }
    out
}
