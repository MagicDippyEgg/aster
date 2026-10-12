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
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
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
    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
    /// Internal completion helper
    #[command(name = "_complete", hide = true)]
    InternalComplete { target: String },
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
    CacheRust {
        #[arg(value_parser = ["true", "false", "yes", "no", "1", "0"])]
        value: String,
    },
}

/// Entry point mirroring the original `main(args)` function.
pub fn run(args: Vec<String>) -> i32 {
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| {
            if arg == "-verbose" {
                "--verbose".to_string()
            } else {
                arg
            }
        })
        .collect();

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
        Commands::Install {
            package,
            yes,
            verbose,
        } => {
            installer.install(&package, yes, verbose)?;
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
        Commands::Completions { shell } => completions(shell),
        Commands::InternalComplete { target } => internal_complete(registry, catalogue, &target),
    }
}

fn internal_complete(
    registry: &RegistryManager,
    catalogue: &CatalogueManager,
    target: &str,
) -> crate::error::Result<i32> {
    match target {
        "available" => {
            let packages = catalogue.search_packages("")?;
            for (pkg_id, _) in packages {
                println!("{pkg_id}");
            }
        }
        "installed" => {
            let installed = registry.list_installed()?;
            for (pkg_id, _) in installed {
                println!("{pkg_id}");
            }
        }
        "repos" => {
            let repos = catalogue.get_repositories()?;
            for (name, _) in repos {
                println!("{name}");
            }
        }
        _ => {}
    }
    Ok(0)
}

fn completions(shell: clap_complete::Shell) -> crate::error::Result<i32> {
    let mut cmd = Cli::command();
    cmd = filter_hidden_subcommands(cmd);

    if shell == clap_complete::Shell::Fish {
        println!("complete -c aster -f");
    }

    clap_complete::generate(shell, &mut cmd, "aster", &mut std::io::stdout());

    if shell == clap_complete::Shell::Fish {
        println!(
            "\n# Custom dynamic completions for Aster package manager\n\
             complete -c aster -n \"__fish_aster_using_subcommand install\" -f -a \"(aster _complete available 2>/dev/null)\"\n\
             complete -c aster -n \"__fish_aster_using_subcommand info\" -f -a \"(aster _complete available 2>/dev/null)\"\n\
             complete -c aster -n \"__fish_aster_using_subcommand installed\" -f -a \"(aster _complete available 2>/dev/null)\"\n\
             complete -c aster -n \"__fish_aster_using_subcommand remove\" -f -a \"(aster _complete installed 2>/dev/null)\"\n\
             complete -c aster -n \"__fish_aster_using_subcommand upgrade\" -f -a \"(aster _complete installed 2>/dev/null)\"\n\
             complete -c aster -n \"__fish_aster_using_subcommand repo; and __fish_seen_subcommand_from remove\" -f -a \"(aster _complete repos 2>/dev/null)\"\n\
             complete -c aster -n \"__fish_aster_using_subcommand repo; and __fish_seen_subcommand_from set-priority\" -f -a \"(aster _complete repos 2>/dev/null)\""
        );
    }
    Ok(0)
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
        let multi_repo = catalogue.get_repositories()?.len() > 1;
        if multi_repo {
            let max_pkg_len = results.keys().map(|k| k.len()).max().unwrap_or(10);
            let pkg_id_width = std::cmp::max(15, max_pkg_len + 2);

            let max_repo_len = results
                .values()
                .map(|info| {
                    info.get("repository")
                        .map(py_str)
                        .unwrap_or_else(|| "default".to_string())
                        .len()
                })
                .max()
                .unwrap_or(4);
            let repo_width = std::cmp::max(8, max_repo_len + 2);

            let max_ptype_len = results
                .values()
                .map(|info| info.get("type").map(py_str).unwrap_or_default().len())
                .max()
                .unwrap_or(4);
            let ptype_width = std::cmp::max(10, max_ptype_len + 2);

            let header = format!(
                "{:<pkg_id_width$} {:<repo_width$} {:<ptype_width$} DESCRIPTION",
                "PACKAGE ID", "REPO", "TYPE"
            );
            let sep_len = std::cmp::max(65, header.len());
            println!("{header}");
            println!("{}", "-".repeat(sep_len));
            for (pkg_id, info) in results.iter() {
                let desc = info.get("description").map(py_str).unwrap_or_default();
                let ptype = info.get("type").map(py_str).unwrap_or_default();
                let repo = info
                    .get("repository")
                    .map(py_str)
                    .unwrap_or_else(|| "default".to_string());
                println!(
                    "{pkg_id:<pkg_id_width$} {repo:<repo_width$} {ptype:<ptype_width$} {desc}"
                );
            }
        } else {
            let max_pkg_len = results.keys().map(|k| k.len()).max().unwrap_or(10);
            let pkg_id_width = std::cmp::max(20, max_pkg_len + 2);

            let max_ptype_len = results
                .values()
                .map(|info| info.get("type").map(py_str).unwrap_or_default().len())
                .max()
                .unwrap_or(4);
            let ptype_width = std::cmp::max(10, max_ptype_len + 2);

            let header = format!(
                "{:<pkg_id_width$} {:<ptype_width$} DESCRIPTION",
                "PACKAGE ID", "TYPE"
            );
            let sep_len = std::cmp::max(65, header.len());
            println!("{header}");
            println!("{}", "-".repeat(sep_len));
            for (pkg_id, info) in results.iter() {
                let desc = info.get("description").map(py_str).unwrap_or_default();
                let ptype = info.get("type").map(py_str).unwrap_or_default();
                println!("{pkg_id:<pkg_id_width$} {ptype:<ptype_width$} {desc}");
            }
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
        if content.trim().is_empty() {
            println!("No history recorded yet.");
        } else {
            print!("{content}");
        }
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
            installer.install(target_pkg, false, false)?;
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
                    installer.install(&pkg_id, false, false)?;
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

fn filter_hidden_subcommands(cmd: clap::Command) -> clap::Command {
    let visible_subcommands: Vec<clap::Command> = cmd
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .map(|sub| filter_hidden_subcommands(sub.clone()))
        .collect();

    let name: &'static str = Box::leak(cmd.get_name().to_string().into_boxed_str());
    let mut new_cmd = clap::Command::new(name);
    if let Some(bin_name) = cmd.get_bin_name() {
        let bn: &'static str = Box::leak(bin_name.to_string().into_boxed_str());
        new_cmd = new_cmd.bin_name(bn);
    }
    if let Some(version) = cmd.get_version() {
        let v: &'static str = Box::leak(version.to_string().into_boxed_str());
        new_cmd = new_cmd.version(v);
    }
    if let Some(about) = cmd.get_about() {
        let a: &'static str = Box::leak(about.to_string().into_boxed_str());
        new_cmd = new_cmd.about(a);
    }
    for arg in cmd.get_arguments() {
        new_cmd = new_cmd.arg(arg.clone());
    }
    for sub in visible_subcommands {
        new_cmd = new_cmd.subcommand(sub);
    }
    new_cmd
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
