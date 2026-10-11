# aster

A small, source-and-binary package manager written in Rust.

## Build

```sh
cargo build --release
```

The binary is produced at `target/release/aster`.

## Usage

```sh
aster repo update          # refresh package indexes
aster search <query>       # search available packages
aster install <package>    # install a package and its dependencies
aster list                 # list installed packages
aster remove <package>     # uninstall a package
aster doctor               # check the installation for problems
```

Packages and configuration live under `~/.bin/aster` by default. Set `ASTER_HOME`
to use a different location.

## Documentation

- [Creating and Managing a Repository](docs/creating-a-repository.md) - A complete guide on setting up custom package repositories, package definition JSON schemas, build systems, and repository hosting.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The integration tests (in `tests/integration.rs`) exercise the library and the
CLI end to end using throwaway homes in temporary directories.
