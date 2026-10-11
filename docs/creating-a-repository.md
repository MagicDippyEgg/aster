# Creating an Aster Package Repository

This document explains how to create, structure, and publish a package repository for the **Aster** package manager.

---

## Overview

An Aster repository is a directory or Git repository hosted on HTTP/HTTPS or accessible locally via file paths. It contains:

1. `index.json`: A catalog listing all available packages in the repository.
2. `packages/`: A directory containing individual JSON definition files for each package.

An example repository layout:

```text
aster-package-repository/
├── index.json
├── packages/
│   ├── fastfetch-bin.json
│   ├── fastfetch-src.json
│   ├── ripgrep-bin.json
│   └── ...
└── README.md
```

---

## 1. The Repository Index (`index.json`)

The `index.json` file serves as the main entry point for Aster to discover packages.

### Schema

```json
{
  "schema_version": 1,
  "packages": {
    "<package_id>": {
      "definition": "packages/<package_id>.json",
      "type": "binary" | "source",
      "description": "Short description of the package"
    }
  }
}
```

### Fields

- `schema_version` (*integer*, required): Must be `1`.
- `packages` (*object*, required): A map where each key is a unique package identifier (e.g. `fastfetch-bin` or `fastfetch-src`), and the value is an object containing:
  - `definition` (*string*, required): Relative path to the package definition file (e.g., `"packages/fastfetch-bin.json"`).
  - `type` (*string*, required): Either `"binary"` or `"source"`.
  - `description` (*string*, optional): A brief summary of the package.

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
        "fastfetch-src": {
            "definition": "packages/fastfetch-src.json",
            "type": "source",
            "description": "An opinionated, fast and lightweight system information tool (source build)"
        }
    }
}
```

---

## 2. Package Definitions (`packages/<id>.json`)

Each package in the repository has a JSON file defining its metadata, version, dependencies, and download or build instructions.

### Common Required Fields

All package definitions must contain the following top-level fields:

- `schema_version` (*integer*, required): Must be `1`.
- `id` (*string*, required): Non-empty string matching the package identifier (e.g., `"fastfetch-bin"`).
- `name` (*string*, required): Human-readable name of the package.
- `version` (*string*, required): Version string (e.g. `"2.69.0"`).
- `type` (*string*, required): Either `"binary"` or `"source"`.

---

### Binary Package Schema (`"type": "binary"`)

Binary package definitions specify precompiled binaries available for download across supported system architectures.

#### Fields

- `downloads` (*object*, required): Map of target platform keys (e.g., `"linux-x86_64"`, `"linux-aarch64"`) to objects containing:
  - `url` (*string*, required): Download URL for the archive (`.tar.gz`, `.tgz`, `.tar.xz`, `.txz`, or `.zip`).
  - `sha256` (*string*, optional): SHA-256 checksum for download verification.
- `supported_platforms` (*array*, optional): List of platform strings supported by this package.
- `homepage` (*string*, optional): Project homepage URL.
- `description` (*string*, optional): Description of the package.
- `dependencies` (*array*, optional): List of dependency package IDs required at runtime.

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

### Source Package Schema (`"type": "source"`)

Source package definitions specify how to fetch source code (e.g. via Git or archive download) and build the binaries locally.

#### Fields

- `source` (*object*, required): Source acquisition details.
  - For Git repositories:
    - `type`: `"git"`
    - `url`: Repository clone URL
    - `ref`: Git tag, branch, or commit hash
  - For archive downloads:
    - `type`: `"archive"` or `"url"`
    - `url`: Download URL
    - `sha256`: SHA-256 checksum
- `build` (*object*, required): Build instructions.
  - `system` (*string*): Supported build systems include `"cmake"`, `"cargo"`, `"make"`, `"meson"`, or `"custom"`.
  - `steps` (*array*, optional): List of shell command strings for custom builds.
- `dependencies` (*array*, optional): Build or runtime dependencies.

#### Source Package Example (`packages/fastfetch-src.json`)

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

## 3. Hosting and Adding a Repository

### Hosting

An Aster repository can be hosted using any HTTP/HTTPS web server or Git hosting provider (such as GitHub, GitLab, or Gitea). The raw contents of `index.json` and the `packages/` directory must be accessible via direct HTTP URLs.

For example, on GitHub:
`https://raw.githubusercontent.com/<username>/<repo_name>/main`

### Adding the Repository to Aster

Users can add your repository to their local Aster installation using the `aster repo add` command:

```sh
aster repo add <repo_name> <repo_url> [--priority <priority>]
```

#### Example

```sh
aster repo add official https://raw.githubusercontent.com/MagicDippyEgg/aster-package-repository/main --priority 100
```

Once added, update repository indexes using:

```sh
aster repo update
```

Now packages from your repository can be searched and installed with `aster search` and `aster install`.
