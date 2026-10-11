# Creating and Managing an Aster Package Repository

This guide provides a complete, in-depth reference for creating, structuring, and hosting a package repository for the **Aster** package manager.

---

## Table of Contents

1. [Overview](#overview)
2. [Repository Directory Layout](#repository-directory-layout)
3. [The Repository Index (`index.json`)](#1-the-repository-index-indexjson)
4. [Package Definition Schemas](#2-package-definition-schemas)
   - [Common Core Fields](#common-core-fields)
   - [Binary Package Definitions (`"type": "binary"`)](#binary-package-definitions-type-binary)
   - [Source Package Definitions (`"type": "source"`)](#source-package-definitions-type-source)
5. [Source Acquisition Types](#3-source-acquisition-types)
   - [Git Repositories](#git-repositories)
   - [Source Archives and HTTP Downloads](#source-archives-and-http-downloads)
6. [Build System Configurations](#4-build-system-configurations)
   - [Cargo (Rust)](#cargo-rust)
   - [CMake](#cmake)
   - [Make](#make)
   - [Custom Scripted Build Steps](#custom-scripted-build-steps)
7. [Dependencies & Executable Resolution](#5-dependencies--executable-resolution)
8. [Hosting a Repository](#6-hosting-a-repository)
9. [Adding and Managing Repositories in Aster](#7-adding-and-managing-repositories-in-aster)

---

## Overview

An Aster repository is an HTTP/HTTPS web service or local directory tree containing:
1. `index.json`: A single JSON catalog listing all available packages, their package types, brief descriptions, and relative file paths to their full package definitions.
2. `packages/`: A subdirectory containing individual JSON package definitions (`<package-id>.json`).

When a user runs `aster repo update`, Aster fetches and caches `index.json`. When installing a package, Aster retrieves that package's JSON definition file on demand.

---

## Repository Directory Layout

An Aster package repository (such as `aster-package-repository`) follows this hierarchy:

```text
aster-package-repository/
├── index.json
├── packages/
│   ├── bat-bin.json
│   ├── fastfetch-bin.json
│   ├── fastfetch-src.json
│   ├── eza-bin.json
│   ├── eza-src.json
│   ├── glow-src.json
│   ├── neofetch-src.json
│   ├── yazi-src.json
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
      "type": "binary",
      "description": "Short description of the package"
    }
  }
}
```

*Note: The `type` field value must be either `"binary"` for precompiled packages or `"source"` for packages built from source code.*

### Field Specifications

- `schema_version` (*integer*, required): Must be `1`.
- `packages` (*object*, required): A map where each key is a unique package identifier (e.g. `fastfetch-bin` or `eza-src`), and the value is an object containing:
  - `definition` (*string*, required): Relative path from the repository root to the package definition file (e.g. `"packages/fastfetch-bin.json"`).
  - `type` (*string*, required): Either `"binary"` (precompiled release) or `"source"` (compiled locally).
  - `description` (*string*, optional): A short summary displayed in `aster search` results.

### Example `index.json`

```json
{
    "schema_version": 1,
    "packages": {
        "bat-bin": {
            "definition": "packages/bat-bin.json",
            "type": "binary",
            "description": "A cat(1) clone with syntax highlighting and Git integration"
        },
        "eza-src": {
            "definition": "packages/eza-src.json",
            "type": "source",
            "description": "A modern, maintained replacement for ls (source build)"
        },
        "fastfetch-src": {
            "definition": "packages/fastfetch-src.json",
            "type": "source",
            "description": "An opinionated, fast and lightweight system information tool (source build)"
        }
    }
}
```

---

## 2. Package Definition Schemas

Each package definition file in `packages/` is a standalone JSON document describing a package's metadata, download locations, source options, and build instructions.

### Common Core Fields

Every package definition (binary or source) must contain these top-level fields:

| Field | Type | Required | Description |
|---|---|---|---|
| `schema_version` | Integer | Yes | Must be `1`. |
| `id` | String | Yes | Unique package identifier matching the key in `index.json` (e.g. `"bat-bin"` or `"eza-src"`). |
| `name` | String | Yes | Human-readable package name (e.g. `"bat"` or `"eza"`). |
| `version` | String | Yes | Release version string (e.g. `"0.26.1"` or `"0.23.5"`). |
| `type` | String | Yes | `"binary"` or `"source"`. |
| `description` | String | No | Detailed package description. |
| `homepage` | String | No | Project website or code repository URL. |
| `dependencies` | Array of Strings | No | List of prerequisite package IDs required prior to installation. |

---

### Binary Package Definitions (`"type": "binary"`)

Binary packages provide pre-built binaries packed inside release archives.

#### Binary Fields

- `downloads` (*object*, required): A dictionary mapping system platform keys to download details.
  - Platform Keys: Standard `<os>-<arch>` strings (e.g., `linux-x86_64`, `linux-aarch64`, `darwin-x86_64`, `darwin-aarch64`).
  - `url` (*string*, required): Direct download link for the release archive (`.tar.gz`, `.tgz`, `.tar.xz`, `.txz`, `.zip`) or executable file.
  - `sha256` (*string*, optional): SHA-256 hash used to verify archive integrity.
- `supported_platforms` (*array of strings*, optional): List of platform keys supported by this binary build.
- `archive.format` (*string*, optional): Explicit archive format (`"tar.gz"`, `"tar.xz"`, `"zip"`). If omitted, format is inferred automatically from file extension.

#### Binary Example (`packages/bat-bin.json`)

```json
{
    "schema_version": 1,
    "id": "bat-bin",
    "name": "bat",
    "version": "0.26.1",
    "type": "binary",
    "description": "A cat(1) clone with syntax highlighting and Git integration",
    "homepage": "https://github.com/sharkdp/bat",
    "downloads": {
        "linux-x86_64": {
            "url": "https://github.com/sharkdp/bat/releases/download/v0.26.1/bat-v0.26.1-x86_64-unknown-linux-musl.tar.gz",
            "sha256": "0dcd8ac79732c0d5b136f11f4ee00e581440e16a44eab5b3105b611bbf2cf191"
        },
        "linux-aarch64": {
            "url": "https://github.com/sharkdp/bat/releases/download/v0.26.1/bat-v0.26.1-aarch64-unknown-linux-musl.tar.gz",
            "sha256": "6369242c584065f195fb20cb36fbd7cb63ae690605bbe89868a7596b596c2c23"
        }
    },
    "supported_platforms": [
        "linux-x86_64",
        "linux-aarch64"
    ]
}
```

---

### Source Package Definitions (`"type": "source"`)

Source package definitions describe how to fetch source code and compile it locally.

#### Source Fields

- `source` (*object*, required): Source retrieval specification (see [Source Acquisition Types](#3-source-acquisition-types)).
- `build` (*object*, required): Compilation settings (see [Build System Configurations](#4-build-system-configurations)).
- `executables` (*array of strings* or *string*, optional): List of target binary names produced by Cargo builds to copy from `target/release/` into the installation directory. *(Note: Applicable to `system: "cargo"` builds; CMake, Make, and custom scripted builds install binaries via staging directory discovery.)*

---

## 3. Source Acquisition Types

The `source` object configures how Aster downloads or clones the source code prior to building.

### Git Repositories

```json
"source": {
    "type": "git",
    "url": "https://github.com/eza-community/eza.git",
    "ref": "v0.23.5"
}
```

- `type`: `"git"`
- `url`: Git repository URL.
- `ref`: Git branch or tag name to check out (passed via `git clone --branch`). Defaults to `"main"` if omitted.

### Source Archives and HTTP Downloads

```json
"source": {
    "type": "tar.gz",
    "url": "https://github.com/dylanaraps/neofetch/archive/refs/tags/7.1.0.tar.gz"
}
```

Supported `type` strings for archives:
- `"tar.gz"`, `"tar.xz"`, `"txz"`, `"zip"`, `"archive"`, `"url"`

---

## 4. Build System Configurations

The `build` object defines how Aster compiles the software.

### Cargo (Rust)

For Rust projects, Aster offers native Cargo integration. If Rust/Cargo is not present on the host system, Aster can download an isolated, cached Rust toolchain automatically.

```json
"build": {
    "system": "cargo",
    "release": true,
    "locked": true
}
```

#### Cargo Options

- `system`: `"cargo"`
- `release` (*boolean*, optional, default `true`): Builds using `cargo build --release`.
- `locked` (*boolean*, optional, default `false`): Appends `--locked` to cargo invocations.
- `cargo_args` (*array of strings*, optional): Replaces all default Cargo invocation arguments (normally `build`, `--release`, etc.). Custom arguments must build the target binaries into `target/release/` or `target/debug/` so Aster can collect them.
- `executables` (*array of strings*, optional): Target binaries produced inside `target/release/` or `target/debug/` to copy into the package staging directory.

#### Cargo Example (`packages/eza-src.json`)

```json
{
    "schema_version": 1,
    "id": "eza-src",
    "name": "eza",
    "version": "0.23.5",
    "type": "source",
    "description": "A modern, maintained replacement for ls (source build)",
    "homepage": "https://eza.rocks",
    "source": {
        "type": "git",
        "url": "https://github.com/eza-community/eza.git",
        "ref": "v0.23.5"
    },
    "build": {
        "system": "cargo",
        "release": true,
        "locked": true
    },
    "executables": [
        "eza"
    ]
}
```

---

### CMake

Configures CMake to build and install into Aster's package staging directory.

```json
"build": {
    "system": "cmake"
}
```

#### CMake Example (`packages/fastfetch-src.json`)

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

Runs `make` followed by `make DESTDIR=... install`.

```json
"build": {
    "system": "make"
}
```

#### Make Example (`packages/neofetch-src.json`)

```json
{
    "schema_version": 1,
    "id": "neofetch-src",
    "name": "neofetch",
    "version": "7.1.0",
    "type": "source",
    "description": "A command-line system information tool written in bash 3.2+ (source build)",
    "homepage": "https://github.com/dylanaraps/neofetch",
    "source": {
        "type": "git",
        "url": "https://github.com/dylanaraps/neofetch.git",
        "ref": "7.1.0"
    },
    "build": {
        "system": "make"
    }
}
```

---

### Custom Scripted Build Steps

For projects built using Go, Autotools, shell scripts, or custom toolchains, define a list of shell command strings under `build.steps`.

*Note: Packages defining custom `build.steps` will prompt for interactive confirmation during `aster install` unless the `-y` or `--yes` flag is supplied.*

```json
"build": {
    "steps": [
        "go build -v -o glow"
    ]
}
```

#### Custom Steps Example (`packages/glow-src.json`)

```json
{
    "schema_version": 1,
    "id": "glow-src",
    "name": "glow",
    "version": "3.0.0",
    "type": "source",
    "description": "Render markdown on the CLI, with pizzazz! (source build)",
    "homepage": "https://github.com/charmbracelet/glow",
    "source": {
        "type": "git",
        "url": "https://github.com/charmbracelet/glow.git",
        "ref": "v3.0.0"
    },
    "build": {
        "steps": [
            "go build -v -o glow"
        ]
    }
}
```

---

## 5. Dependencies & Executable Resolution

### Dependency Resolution

Packages can specify runtime or build dependencies using the `dependencies` array:

```json
"dependencies": [
    "openssl-bin"
]
```

Aster automatically resolves dependencies recursively and installs missing prerequisites prior to building or installing the target package.

### Executable Link Discovery

When a package installation completes:
1. For Cargo (`system: "cargo"`) builds, if `executables` is declared in the JSON definition, Aster copies those target binaries from `target/release/` into the staging directory.
2. For all build systems (CMake, Make, custom build steps, and binary archives), Aster discovers executable files installed under conventional staging directories like `bin/`, `sbin/`, `usr/bin/`, or `usr/local/bin/` and symlinks them into `~/.bin/aster/bin/`.

---

## 6. Hosting a Repository

An Aster package repository can be hosted on any static web server or Git hosting platform (such as GitHub, GitLab, Sourcehut, or Gitea).

### Hosting on GitHub

1. Create a public repository (e.g. `my-user/aster-package-repository`).
2. Add `index.json` and package definition files inside `packages/`.
3. Obtain the raw base URL for your repository branch:
   `https://raw.githubusercontent.com/my-user/aster-package-repository/main`

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
aster repo add local-dev file:///path/to/local/aster-package-repository --priority 200
```

### Repository Priorities

Repository priority defaults to `100`. When multiple repositories offer a package with the same `id`, the repository with the highest priority score takes precedence during package resolution and installation.

### Refreshing Repository Catalogs

```sh
aster repo update
```

### Removing a Repository

```sh
aster repo remove <repo-name>
```
