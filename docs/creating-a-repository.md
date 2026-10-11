# Creating and Managing an Aster Package Repository

This guide provides a complete, in-depth reference for creating, structuring, and hosting a package repository for the **Aster** package manager.

---

## Table of Contents

1. [Overview](#overview)
2. [Repository Layout](#repository-layout)
3. [The Repository Index (`index.json`)](#1-the-repository-index-indexjson)
4. [Package Definition Schemas](#2-package-definition-schemas)
   - [Common Fields](#common-fields)
   - [Binary Packages (`"type": "binary"`)](#binary-packages-type-binary)
   - [Source Packages (`"type": "source"`)](#source-packages-type-source)
5. [Source Acquisition Types](#3-source-acquisition-types)
   - [Git Repositories](#git-repositories)
   - [Archives and File Downloads](#archives-and-file-downloads)
6. [Build Systems and Custom Steps](#4-build-systems-and-custom-steps)
   - [Cargo Build System](#cargo-rust)
   - [CMake Build System](#cmake)
   - [Make Build System](#make)
   - [Custom Scripted Steps](#custom-scripted-steps)
7. [Dependencies & Executable Resolution](#5-dependencies--executable-resolution)
8. [Hosting a Repository](#6-hosting-a-repository)
9. [Adding and Managing Repositories in Aster](#7-adding-and-managing-repositories-in-aster)

---

## Overview

An Aster repository is an HTTP/HTTPS web service or a local directory tree containing:
1. `index.json`: A single JSON catalog listing all available packages, their package types, brief descriptions, and relative paths to their full package definitions.
2. `packages/`: A subdirectory containing individual JSON package definitions (`<package-id>.json`).

When a user runs `aster repo update`, Aster downloads and caches `index.json`. When installing a specific package, Aster fetches that package's JSON definition on demand.

---

## Repository Layout

A typical Aster package repository structured like `aster-package-repository` follows this hierarchy:

```text
aster-package-repository/
├── index.json
├── packages/
│   ├── fastfetch-bin.json
│   ├── fastfetch-src.json
│   ├── eza-bin.json
│   ├── eza-src.json
│   ├── tree-src.json
│   ├── jq-src.json
│   └── ...
└── README.md
```

---

## 1. The Repository Index (`index.json`)

The `index.json` file resides at the root of the repository.

### Schema

```json
{
  "schema_version": 1,
  "packages": {
    "<package_id>": {
      "definition": "packages/<package_id>.json",
      "type": "binary" | "source",
      "description": "Brief description of the package"
    }
  }
}
```

### Field Specifications

- `schema_version` (*integer*, required): Must be `1`.
- `packages` (*object*, required): Map of unique package IDs to metadata entries.
  - `definition` (*string*, required): Relative path to the package JSON file (e.g., `"packages/fastfetch-bin.json"`).
  - `type` (*string*, required): `"binary"` for precompiled binaries or `"source"` for source builds.
  - `description` (*string*, optional): A short summary displayed in `aster search` results.

### Example `index.json`

```json
{
    "schema_version": 1,
    "packages": {
        "fastfetch-bin": {
            "definition": "packages/fastfetch-bin.json",
            "type": "binary",
            "description": "An opinionated, fast and lightweight system information tool"
        },
        "eza-src": {
            "definition": "packages/eza-src.json",
            "type": "source",
            "description": "A modern, maintained replacement for ls (source build)"
        },
        "jq-src": {
            "definition": "packages/jq-src.json",
            "type": "source",
            "description": "Command-line JSON processor (source build)"
        }
    }
}
```

---

## 2. Package Definition Schemas

Every JSON definition file in `packages/` contains the full specification for a package.

### Common Fields

All package definitions require these top-level fields:

| Field | Type | Required | Description |
|---|---|---|---|
| `schema_version` | Integer | Yes | Must be `1`. |
| `id` | String | Yes | Non-empty string matching the package key in `index.json`. |
| `name` | String | Yes | Human-readable package name (e.g. `"Fastfetch"` or `"eza"`). |
| `version` | String | Yes | Package release version (e.g. `"2.69.0"` or `"0.18.2"`). |
| `type` | String | Yes | `"binary"` or `"source"`. |
| `description` | String | No | Detailed package description. |
| `homepage` | String | No | URL to the project website or repository. |
| `dependencies` | Array of Strings | No | Package IDs of runtime or build dependencies. |

---

### Binary Packages (`"type": "binary"`)

Binary package definitions specify download locations for precompiled release archives across target architectures.

#### Fields

- `downloads` (*object*, required): A mapping from target platform keys to download metadata objects.
  - Platform Key Format: `<os>-<arch>` (e.g., `linux-x86_64`, `linux-aarch64`, `darwin-x86_64`, `darwin-aarch64`).
  - `url` (*string*, required): Download URL for the archive or executable (`.tar.gz`, `.tgz`, `.tar.xz`, `.txz`, `.zip`, or direct binary download).
  - `sha256` (*string*, optional): SHA-256 hash for checksum validation.
- `supported_platforms` (*array of strings*, optional): List of platform keys supported by this package (e.g., `["linux-x86_64", "linux-aarch64"]`).
- `archive.format` (*string*, optional): Explicit archive format (`"tar.gz"`, `"tar.xz"`, `"zip"`). Inferred automatically from URL extensions if omitted.

#### Binary Package Example (`packages/fastfetch-bin.json`)

```json
{
    "schema_version": 1,
    "id": "fastfetch-bin",
    "name": "Fastfetch",
    "version": "2.69.0",
    "type": "binary",
    "description": "An opinionated, fast and lightweight system information tool",
    "homepage": "https://github.com/fastfetch-cli/fastfetch",
    "downloads": {
        "linux-x86_64": {
            "url": "https://github.com/fastfetch-cli/fastfetch/releases/download/2.69.0/fastfetch-linux-amd64.tar.gz",
            "sha256": "9fe880a34de3fec88e57a69230c02fd7be0846db3f3ea9f88f2b74489a79ff55"
        },
        "linux-aarch64": {
            "url": "https://github.com/fastfetch-cli/fastfetch/releases/download/2.69.0/fastfetch-linux-aarch64.tar.gz",
            "sha256": "843a0d4e3d604efc7cce18df1efcc09b00a1960cfea9949118b50bf999694027"
        }
    },
    "supported_platforms": [
        "linux-x86_64",
        "linux-aarch64"
    ]
}
```

---

### Source Packages (`"type": "source"`)

Source package definitions specify how Aster retrieves source code and compiles it locally.

#### Core Fields

- `source` (*object*, required): Source code retrieval settings (see [Source Acquisition Types](#3-source-acquisition-types)).
- `build` (*object*, required): Build configuration (see [Build Systems](#4-build-systems-and-custom-steps)).
- `executables` (*array of strings* or *string*, optional): List of specific executable names produced by the build to symlink into Aster's binary directory.

---

## 3. Source Acquisition Types

The `source` object supports multiple methods for obtaining source code:

### Git Repositories

```json
"source": {
    "type": "git",
    "url": "https://github.com/eza-community/eza.git",
    "ref": "v0.18.2"
}
```

- `type`: `"git"`
- `url`: Git repository clone URL (`https://...` or `git@...`).
- `ref`: Tag, branch, or commit hash to checkout (defaults to `"main"` if omitted).

### Archives and File Downloads

Source archives can be fetched via tarballs or zip files:

```json
"source": {
    "type": "tar.gz",
    "url": "https://github.com/pking543/tree/archive/refs/tags/2.1.1.tar.gz"
}
```

Supported `type` values for archives:
- `"tar.gz"`, `"tar.xz"`, `"txz"`, `"zip"`, `"archive"`, `"url"`

---

## 4. Build Systems and Custom Steps

The `build` object configures how Aster compiles the package.

### Cargo (Rust)

Aster has native integration with Cargo. If Rust is not installed on the host system, Aster can download an isolated, cached Rust toolchain automatically.

```json
"build": {
    "system": "cargo"
}
```

#### Cargo Options

- `release` (*boolean*, optional, default `true`): Builds with `--release`.
- `locked` (*boolean*, optional, default `false`): Passes `--locked` to cargo build.
- `cargo_args` (*array of strings*, optional): Replaces default `cargo build --release` flags with custom arguments.
- `executables` (*array of strings*, optional): Names of target binaries produced inside `target/release/`.

#### Example: Cargo Source Package (`packages/yazi-src.json`)

```json
{
    "schema_version": 1,
    "id": "yazi-src",
    "name": "Yazi",
    "version": "25.2.26",
    "type": "source",
    "description": "Blazing fast terminal file manager written in Rust (source build)",
    "homepage": "https://github.com/sxyazi/yazi",
    "source": {
        "type": "git",
        "url": "https://github.com/sxyazi/yazi.git",
        "ref": "v25.2.26"
    },
    "build": {
        "system": "cargo"
    },
    "executables": [
        "yazi",
        "ya"
    ]
}
```

---

### CMake

Invokes CMake to configure, build, and install into Aster's staging directory.

```json
"build": {
    "system": "cmake"
}
```

#### Example: CMake Source Package (`packages/fastfetch-src.json`)

```json
{
    "schema_version": 1,
    "id": "fastfetch-src",
    "name": "Fastfetch",
    "version": "2.69.0",
    "type": "source",
    "description": "An opinionated, fast and lightweight system information tool (source build)",
    "homepage": "https://github.com/fastfetch-cli/fastfetch",
    "source": {
        "type": "git",
        "url": "https://github.com/fastfetch-cli/fastfetch.git",
        "ref": "2.69.0"
    },
    "build": {
        "system": "cmake"
    }
}
```

---

### Make

Invokes `make` followed by `make DESTDIR=... install`.

```json
"build": {
    "system": "make"
}
```

#### Example: Make Source Package (`packages/tree-src.json`)

```json
{
    "schema_version": 1,
    "id": "tree-src",
    "name": "tree",
    "version": "2.2.1",
    "type": "source",
    "description": "Recursive directory listing program (source build)",
    "homepage": "https://oldmanhook.github.io/unix-tree/",
    "source": {
        "type": "git",
        "url": "https://github.com/oldmanhook/unix-tree.git",
        "ref": "2.2.1"
    },
    "build": {
        "system": "make"
    }
}
```

---

### Custom Scripted Steps

For builds requiring custom commands (e.g. Go builds, Autotools, shell scripts), specify an array of shell command strings under `build.steps`.

*Note: Custom build steps require interactive confirmation during `aster install` unless the `-y` or `--yes` flag is provided.*

```json
"build": {
    "steps": [
        "autoreconf -i",
        "./configure --prefix=$ASTER_HOME/packages/jq-src",
        "make -j$(nproc)",
        "make install"
    ]
}
```

#### Example: Custom Go Build Steps (`packages/jq-src.json` / `packages/glow-src.json`)

```json
{
    "schema_version": 1,
    "id": "glow-src",
    "name": "Glow",
    "version": "2.1.0",
    "type": "source",
    "description": "Render markdown on the CLI, with pizzazz! (source build)",
    "homepage": "https://github.com/charmbracelet/glow",
    "source": {
        "type": "git",
        "url": "https://github.com/charmbracelet/glow.git",
        "ref": "v2.1.0"
    },
    "build": {
        "steps": [
            "go build -o glow"
        ]
    }
}
```

---

## 5. Dependencies & Executable Resolution

### Dependencies

Declare runtime or build dependencies in the `dependencies` array:

```json
"dependencies": [
    "openssl-bin"
]
```

Aster resolves and installs dependencies before installing the target package.

### Executable Resolution

When a package is installed:
1. Aster inspects `executables` in the package definition. If specified, only those named binaries are symlinked into `~/.bin/aster/bin/`.
2. For binary archives or source builds without explicit `executables`, Aster discovers executables placed inside `bin/` or `usr/bin/` within the package staging directory.

---

## 6. Hosting a Repository

An Aster repository can be hosted on any static HTTP/HTTPS web server or Git host (such as GitHub, GitLab, Sourcehut, or Gitea).

### Hosting on GitHub

1. Create a repository on GitHub (e.g. `my-user/aster-repo`).
2. Push `index.json` and your `packages/*.json` files to the `main` branch.
3. Use the raw content base URL:
   `https://raw.githubusercontent.com/my-user/aster-repo/main`

---

## 7. Adding and Managing Repositories in Aster

### Adding a Remote Repository

```sh
aster repo add <repo-name> <url> [--priority <priority>]
```

#### Example

```sh
aster repo add official https://raw.githubusercontent.com/MagicDippyEgg/aster-package-repository/main --priority 100
```

### Adding a Local Repository

```sh
aster repo add my-local file:///home/user/my-aster-repo --priority 200
```

### Repository Priority

Repositories have a priority value (default: `100`). Higher values take precedence when resolving packages with identical IDs across multiple repositories.

### Updating Repository Catalogs

To pull the latest index files from all configured repositories:

```sh
aster repo update
```

### Removing a Repository

```sh
aster repo remove <repo-name>
```
