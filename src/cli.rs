//! Command Line Interface for Aster Package Manager.

use crate::catalogue::{display_priority, CatalogueManager};
use crate::config::AsterConfig;
use crate::installer::PackageInstaller;
use crate::registry::RegistryManager;
use crate::util::py_str;
use crate::version::compare_versions;
use clap::{CommandFactory, Parser, Subcommand};
use serde_json::{json, Map, Value};
use std::fs;

#[derive(Parser)]
#[command(
    name = "aster",
    version,
    about = "Aster Package Manager - Independent Linux Package Manager"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List installed packages
    List,
    /// Search remote package catalogues
    Search {
        #[arg(default_value = "")]
        query: String,
    },
    /// Show metadata for a package
    Info { package: String },
    /// Check if package is installed
    Installed { package: String },
    /// Install a package
    Install {
        package: String,
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// Remove an installed package
    Remove {
        package: String,
        #[arg(long)]
        force: bool,
    },
    /// Clear temporary build and download caches
    Clean,
    /// Diagnose installation, path, and tool issues
    Doctor,
    /// Show history log of package operations
    History,
    /// Refresh package indexes and show available updates
    Update,
    /// Upgrade installed packages
    Upgrade {
        #[arg(default_value = "")]
        package: String,
    },
    /// Manage package repositories
    Repo {
        #[command(subcommand)]
        command: Option<RepoCommand>,
    },
    /// View or modify configuration
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
}

#[derive(Subcommand)]
enum RepoCommand {
    /// List configured repositories
    List,
    /// Update cached package indexes
    Update,
    /// Add a package repository
    Add {
        name: String,
        url: String,
        #[arg(short = 'p', long, default_value_t = 100)]
        priority: i64,
    },
    /// Remove a package repository
    Remove { name: String },
    /// Set priority for a package repository
    SetPriority { name: String, priority: i64 },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Show current configuration
    Show,
    /// Set Rust toolchain caching preference
    CacheRust { value: String },
}

/// Entry point mirroring the original `main(args)` function.
pub fn run(args: Vec<String>) -> i32 {
    let cli = match Cli::try_parse_from(std::iter::once("aster".to_string()).chain(args)) {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return match err.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
                _ => 2,
            };
        }
    };

    if cli.command.is_none() {
        let mut cmd = Cli::command();
        let _ = cmd.print_help();
        println!();
        return 0;
    }

    let config = AsterConfig::new();
    let registry = match RegistryManager::new(config.clone()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    let catalogue = match CatalogueManager::new(config.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    let installer = PackageInstaller::new(config.clone(), registry.clone(), catalogue.clone());

    let result = dispatch(
        cli.command.unwrap(),
        &config,
        &registry,
        &catalogue,
        &installer,
    );
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

fn dispatch(
    command: Commands,
    config: &AsterConfig,
    registry: &RegistryManager,
    catalogue: &CatalogueManager,
    installer: &PackageInstaller,
) -> crate::error::Result<i32> {
    match command {
        Commands::List => list(registry),
        Commands::Search { query } => search(catalogue, &query),
        Commands::Info { package } => info(registry, catalogue, &package),
        Commands::Installed { package } => installed(registry, &package),
        Commands::Install { package, yes } => {
            installer.install(&package, yes)?;
            Ok(0)
        }
        Commands::Remove { package, force } => {
            installer.remove(&package, force)?;
            Ok(0)
        }
        Commands::Clean => clean(config),
        Commands::Doctor => doctor(config),
        Commands::History => history(config),
        Commands::Update => update(registry, catalogue),
        Commands::Upgrade { package } => upgrade(registry, catalogue, installer, &package),
        Commands::Repo { command } => repo(config, catalogue, command),
        Commands::Config { command } => config_cmd(config, command),
    }
}

fn list(registry: &RegistryManager) -> crate::error::Result<i32> {
    let installed = registry.list_installed()?;
    if installed.is_empty() {
        println!("No packages currently installed.");
    } else {
        println!(
            "{:<20} {:<15} {:<10} REPOSITORY",
            "PACKAGE ID", "VERSION", "TYPE"
        );
        println!("{}", "-".repeat(60));
        for (pkg_id, info) in installed.iter() {
            let version = info
                .get("version")
                .map(py_str)
                .unwrap_or_else(|| "n/a".to_string());
            let ptype = info
                .get("type")
                .map(py_str)
                .unwrap_or_else(|| "n/a".to_string());
            let repo = info
                .get("source_repository")
                .map(py_str)
                .unwrap_or_else(|| "default".to_string());
            println!("{pkg_id:<20} {version:<15} {ptype:<10} {repo}");
        }
    }
    Ok(0)
}

fn search(catalogue: &CatalogueManager, query: &str) -> crate::error::Result<i32> {
    let results = catalogue.search_packages(query)?;
    if results.is_empty() {
        println!("No packages found matching '{query}'.");
    } else {
        println!("{:<20} {:<10} DESCRIPTION", "PACKAGE ID", "TYPE");
        println!("{}", "-".repeat(65));
        for (pkg_id, info) in results.iter() {
            let desc = info.get("description").map(py_str).unwrap_or_default();
            let ptype = info.get("type").map(py_str).unwrap_or_default();
            println!("{pkg_id:<20} {ptype:<10} {desc}");
        }
    }
    Ok(0)
}

fn info(
    registry: &RegistryManager,
    catalogue: &CatalogueManager,
    pkg_id: &str,
) -> crate::error::Result<i32> {
    let installed_info = registry.get_installed_package(pkg_id)?;
    let pkg_def = catalogue.get_package_definition(pkg_id, None)?;

    if installed_info.is_none() && pkg_def.is_none() {
        println!("Package '{pkg_id}' not found in registry or catalogue.");
        return Ok(1);
    }

    println!("Package: {pkg_id}");
    if let Some(ii) = &installed_info {
        println!(
            "  Installed Version: {}",
            ii.get("version")
                .map(py_str)
                .unwrap_or_else(|| "None".to_string())
        );
        println!(
            "  Installation Dir:  {}",
            ii.get("install_dir")
                .map(py_str)
                .unwrap_or_else(|| "None".to_string())
        );
    }
    if let Some(def) = &pkg_def {
        println!(
            "  Name:        {}",
            def.get("name")
                .map(py_str)
                .unwrap_or_else(|| "None".to_string())
        );
        println!(
            "  Version:     {}",
            def.get("version")
                .map(py_str)
                .unwrap_or_else(|| "None".to_string())
        );
        println!(
            "  Type:        {}",
            def.get("type")
                .map(py_str)
                .unwrap_or_else(|| "None".to_string())
        );
        println!(
            "  Description: {}",
            def.get("description")
                .map(py_str)
                .unwrap_or_else(|| "N/A".to_string())
        );
        println!(
            "  Homepage:    {}",
            def.get("homepage")
                .map(py_str)
                .unwrap_or_else(|| "N/A".to_string())
        );
    }
    Ok(0)
}

fn installed(registry: &RegistryManager, pkg_id: &str) -> crate::error::Result<i32> {
    if registry.is_installed(pkg_id)? {
        let info = registry.get_installed_package(pkg_id)?;
        let version = info
            .as_ref()
            .and_then(|v| v.get("version"))
            .map(py_str)
            .unwrap_or_else(|| "None".to_string());
        println!("{pkg_id} is installed (version {version}).");
        Ok(0)
    } else {
        println!("{pkg_id} is not installed.");
        Ok(1)
    }
}

fn clean(config: &AsterConfig) -> crate::error::Result<i32> {
    for path in [
        &config.downloads_cache,
        &config.archives_cache,
        &config.build_dir,
    ] {
        if path.exists() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let item = entry.path();
                if item.is_file() {
                    fs::remove_file(&item)?;
                } else if item.is_dir() {
                    fs::remove_dir_all(&item)?;
                }
            }
        }
    }
    println!("Successfully cleared build and download caches.");
    Ok(0)
}

fn doctor(config: &AsterConfig) -> crate::error::Result<i32> {
    println!("Aster Doctor - Diagnostic Check");
    println!("{}", "=".repeat(40));
    println!("Aster Home: {}", config.root_dir.display());

    let path_env = std::env::var("PATH").unwrap_or_default();
    let aster_bin_str = config.bin_dir.to_string_lossy().to_string();
    let aster_home_bin_str = config.root_dir.to_string_lossy().to_string();

    let bin_in_path = path_env.contains(&aster_bin_str);
    let home_bin_in_path = path_env.contains(&aster_home_bin_str);

    println!(
        "Directory {} in PATH: {}",
        config.bin_dir.display(),
        if bin_in_path { "YES" } else { "NO" }
    );
    if !bin_in_path {
        println!(
            "  Warning: Add '{}' to your PATH to run installed commands.",
            config.bin_dir.display()
        );
    }

    println!(
        "Directory {} in PATH: {}",
        config.root_dir.display(),
        if home_bin_in_path { "YES" } else { "NO" }
    );
    if !home_bin_in_path {
        println!(
            "  Warning: Add '{}' to your PATH to run 'aster' executable.",
            config.root_dir.display()
        );
    }

    let tools = ["git", "cmake", "make", "gcc", "tar", "unzip"];
    println!("\nBuild Tools Availability:");
    for tool in tools {
        match crate::util::which(tool, None) {
            Some(found) => println!("  {tool:<10}: FOUND ({})", found.display()),
            None => println!("  {tool:<10}: NOT FOUND"),
        }
    }

    Ok(0)
}

fn history(config: &AsterConfig) -> crate::error::Result<i32> {
    let history_file = config.logs_dir.join("history.log");
    if !history_file.exists() {
        println!("No history recorded yet.");
    } else {
        let content = fs::read_to_string(history_file)?;
        println!("{content}");
    }
    Ok(0)
}

fn update(registry: &RegistryManager, catalogue: &CatalogueManager) -> crate::error::Result<i32> {
    println!("Refreshing package indexes...");
    catalogue.update_all()?;
    let installed = registry.list_installed()?;
    let mut updates_found: Vec<(String, String, String)> = Vec::new();
    for (pkg_id, inst_info) in installed.iter() {
        if let Some(pkg_def) = catalogue.get_package_definition(pkg_id, None)? {
            let avail_ver = pkg_def.get("version").map(py_str).unwrap_or_default();
            let inst_ver = inst_info.get("version").map(py_str).unwrap_or_default();
            if compare_versions(&inst_ver, &avail_ver) < 0 {
                updates_found.push((pkg_id.clone(), inst_ver, avail_ver));
            }
        }
    }

    if !updates_found.is_empty() {
        println!("\nAvailable package updates:");
        println!("{:<20} {:<15} AVAILABLE", "PACKAGE ID", "INSTALLED");
        println!("{}", "-".repeat(50));
        for (pkg_id, inst_v, avail_v) in &updates_found {
            println!("{pkg_id:<20} {inst_v:<15} {avail_v}");
        }
        println!("\nRun 'aster upgrade' to upgrade all packages.");
    } else {
        println!("All installed packages are up to date.");
    }
    Ok(0)
}

fn upgrade(
    registry: &RegistryManager,
    catalogue: &CatalogueManager,
    installer: &PackageInstaller,
    target_pkg: &str,
) -> crate::error::Result<i32> {
    catalogue.update_all()?;

    if !target_pkg.is_empty() {
        if !registry.is_installed(target_pkg)? {
            println!("Package '{target_pkg}' is not installed.");
            return Ok(1);
        }
        let inst_info = registry.get_installed_package(target_pkg)?;
        let pkg_def = catalogue.get_package_definition(target_pkg, None)?;
        let pkg_def = match pkg_def {
            Some(d) => d,
            None => {
                println!("No definition found for '{target_pkg}' in catalogue.");
                return Ok(1);
            }
        };
        let inst_ver = inst_info
            .as_ref()
            .and_then(|v| v.get("version"))
            .map(py_str)
            .unwrap_or_default();
        let avail_ver = pkg_def.get("version").map(py_str).unwrap_or_default();
        if compare_versions(&inst_ver, &avail_ver) < 0 {
            println!("Upgrading '{target_pkg}' from {inst_ver} to {avail_ver}...");
            installer.remove(target_pkg, false)?;
            installer.install(target_pkg, false)?;
        } else {
            println!("Package '{target_pkg}' is already at latest version ({inst_ver}).");
        }
    } else {
        let installed = registry.list_installed()?;
        let keys: Vec<String> = installed.keys().cloned().collect();
        let mut upgraded_any = false;
        for pkg_id in keys {
            if let Some(pkg_def) = catalogue.get_package_definition(&pkg_id, None)? {
                let inst_ver = installed
                    .get(&pkg_id)
                    .and_then(|v| v.get("version"))
                    .map(py_str)
                    .unwrap_or_default();
                let avail_ver = pkg_def.get("version").map(py_str).unwrap_or_default();
                if compare_versions(&inst_ver, &avail_ver) < 0 {
                    println!("Upgrading '{pkg_id}' from {inst_ver} to {avail_ver}...");
                    installer.remove(&pkg_id, false)?;
                    installer.install(&pkg_id, false)?;
                    upgraded_any = true;
                }
            }
        }
        if !upgraded_any {
            println!("All packages are already up to date.");
        }
    }
    Ok(0)
}

fn repo(
    config: &AsterConfig,
    catalogue: &CatalogueManager,
    command: Option<RepoCommand>,
) -> crate::error::Result<i32> {
    let mut repos_data = config.load_json(&config.repositories_json)?;
    let mut repos: Map<String, Value> = repos_data
        .get("repositories")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    match command {
        None => {
            let mut cmd = Cli::command();
            if let Some(sub) = cmd.find_subcommand_mut("repo") {
                let _ = sub.print_help();
            }
            println!();
            Ok(0)
        }
        Some(RepoCommand::List) => {
            println!("{:<15} {:<10} URL", "NAME", "PRIORITY");
            println!("{}", "-".repeat(65));
            for (name, info) in catalogue.get_sorted_repositories()? {
                let prio = display_priority(&info);
                let url = info.get("url").map(py_str).unwrap_or_default();
                println!("{name:<15} {prio:<10} {url}");
            }
            Ok(0)
        }
        Some(RepoCommand::Add {
            name,
            url,
            priority,
        }) => {
            let mut entry = Map::new();
            entry.insert("name".to_string(), json!(name));
            entry.insert("url".to_string(), json!(url));
            entry.insert("priority".to_string(), json!(priority));
            repos.insert(name.clone(), Value::Object(entry));
            repos_data.insert("repositories".to_string(), Value::Object(repos));
            config.save_json_atomic(&config.repositories_json, &Value::Object(repos_data))?;
            println!("Repository '{name}' added successfully with priority {priority}.");
            Ok(0)
        }
        Some(RepoCommand::SetPriority { name, priority }) => {
            if let Some(entry) = repos.get_mut(&name).and_then(|v| v.as_object_mut()) {
                entry.insert("priority".to_string(), json!(priority));
                repos_data.insert("repositories".to_string(), Value::Object(repos));
                config.save_json_atomic(&config.repositories_json, &Value::Object(repos_data))?;
                println!("Set priority of repository '{name}' to {priority}.");
                Ok(0)
            } else {
                println!("Repository '{name}' not found.");
                Ok(1)
            }
        }
        Some(RepoCommand::Remove { name }) => {
            if repos.remove(&name).is_some() {
                repos_data.insert("repositories".to_string(), Value::Object(repos));
                config.save_json_atomic(&config.repositories_json, &Value::Object(repos_data))?;
                println!("Repository '{name}' removed successfully.");
            } else {
                println!("Repository '{name}' not found.");
            }
            Ok(0)
        }
        Some(RepoCommand::Update) => {
            println!("Updating package catalogue indexes...");
            catalogue.update_all()?;
            println!("Catalogue update complete.");
            Ok(0)
        }
    }
}

fn config_cmd(config: &AsterConfig, command: Option<ConfigCommand>) -> crate::error::Result<i32> {
    let mut cfg_data = config.load_json(&config.config_json)?;

    match command {
        Some(ConfigCommand::CacheRust { value }) => {
            let lower = value.to_lowercase();
            let val = lower == "true" || lower == "yes" || lower == "1";
            cfg_data.insert("cache_rust_toolchain".to_string(), Value::Bool(val));
            config.save_json_atomic(&config.config_json, &Value::Object(cfg_data))?;
            println!(
                "Set 'cache_rust_toolchain' to {}.",
                if val { "True" } else { "False" }
            );
            if !val && config.rust_toolchain_dir.exists() {
                let _ = fs::remove_dir_all(&config.rust_toolchain_dir);
                println!("Cleaned cached Rust toolchain directory.");
            }
            Ok(0)
        }
        _ => {
            println!("Aster Configuration:");
            println!("{}", "-".repeat(40));
            for (k, v) in cfg_data.iter() {
                println!("  {k:<25}: {}", py_str(v));
            }
            Ok(0)
        }
    }
}
