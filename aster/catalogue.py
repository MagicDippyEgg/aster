"""
Catalogue repository management and fetching.
"""

import os
import json
import urllib.request
import urllib.error
from pathlib import Path
from typing import Dict, Any, Optional
from aster.config import AsterConfig, fetch_url
from aster.schema import validate_index, validate_package_definition

class CatalogueManager:
    def __init__(self, config: AsterConfig):
        self.config = config
        self.config.ensure_directories()

    def get_repositories(self) -> Dict[str, Any]:
        """Loads configured catalogue repositories from repositories.json."""
        data = self.config.load_json(self.config.repositories_json)
        return data.get("repositories", {})

    def update_repository(self, repo_name: str = "default") -> dict:
        """
        Fetches remote index.json for repo_name and caches it locally in repository-cache/<repo_name>/index.json.
        """
        repos = self.get_repositories()
        if repo_name not in repos:
            raise ValueError(f"Repository '{repo_name}' is not configured.")

        repo_info = repos[repo_name]
        repo_url = repo_info["url"].rstrip("/")
        index_url = f"{repo_url}/index.json"

        cache_dir = self.config.repository_cache / repo_name
        cache_dir.mkdir(parents=True, exist_ok=True)
        cached_index_file = cache_dir / "index.json"

        if repo_url.startswith("http://") or repo_url.startswith("https://"):
            try:
                raw_bytes = fetch_url(index_url, timeout=15)
                data = json.loads(raw_bytes.decode("utf-8"))
            except Exception as e:
                raise RuntimeError(f"Failed to fetch repository index from {index_url}: {e}")
        elif repo_url.startswith("file://"):
            local_path = Path(repo_url[7:]) / "index.json"
            if not local_path.exists():
                raise FileNotFoundError(f"Local catalogue index file not found: {local_path}")
            with open(local_path, "r", encoding="utf-8") as f:
                data = json.load(f)
        else:
            # Local filesystem directory path
            local_path = Path(repo_url) / "index.json"
            if not local_path.exists():
                raise FileNotFoundError(f"Local catalogue index file not found: {local_path}")
            with open(local_path, "r", encoding="utf-8") as f:
                data = json.load(f)

        validate_index(data)
        self.config.save_json_atomic(cached_index_file, data)

        # Clear cached package definition files for this repository so new definitions are retrieved
        pkgs_cache_dir = cache_dir / "packages"
        if pkgs_cache_dir.exists():
            import shutil
            shutil.rmtree(pkgs_cache_dir)

        return data

    def update_all(self):
        """Updates all configured repositories."""
        repos = self.get_repositories()
        for repo_name in repos:
            self.update_repository(repo_name)

    def get_cached_index(self, repo_name: str = "default") -> Optional[dict]:
        """Returns cached index.json for repo_name if present."""
        cached_index_file = self.config.repository_cache / repo_name / "index.json"
        if not cached_index_file.exists():
            return None
        data = self.config.load_json(cached_index_file)
        return data

    def search_packages(self, query: str = "") -> Dict[str, dict]:
        """
        Searches across cached indexes of all repositories.
        Returns a dict mapping package_id to summary metadata.
        """
        import re
        query_lower = query.lower().strip() if query else ""
        desc_pattern = re.compile(r'\b' + re.escape(query_lower) + r'\b', re.IGNORECASE) if query_lower else None

        results = {}
        repos = self.get_repositories()
        for repo_name in repos:
            index = self.get_cached_index(repo_name)
            if not index:
                continue
            pkgs = index.get("packages", {})
            for pkg_id, pkg_info in pkgs.items():
                if not query_lower:
                    matches = True
                elif query_lower in pkg_id.lower():
                    matches = True
                elif desc_pattern and desc_pattern.search(pkg_info.get("description", "")):
                    matches = True
                else:
                    matches = False

                if matches:
                    entry = dict(pkg_info)
                    entry["repository"] = repo_name
                    results[pkg_id] = entry
        return results

    def get_package_definition(self, package_id: str, repo_name: Optional[str] = None) -> Optional[dict]:
        """
        Fetches package definition JSON for package_id.
        First checks cached files, or fetches from repository URL.
        """
        repos = self.get_repositories()
        target_repos = [repo_name] if repo_name else list(repos.keys())

        for rname in target_repos:
            if rname not in repos:
                continue
            index = self.get_cached_index(rname)
            if not index or package_id not in index.get("packages", {}):
                continue

            pkg_entry = index["packages"][package_id]
            def_path = pkg_entry.get("definition", f"packages/{package_id}.json")

            cached_def_file = self.config.repository_cache / rname / def_path
            if cached_def_file.exists():
                data = self.config.load_json(cached_def_file)
                validate_package_definition(data)
                return data

            # Fetch definition from repo URL
            repo_url = repos[rname]["url"].rstrip("/")
            def_url = f"{repo_url}/{def_path}"

            if repo_url.startswith("http://") or repo_url.startswith("https://"):
                try:
                    raw_bytes = fetch_url(def_url, timeout=15)
                    data = json.loads(raw_bytes.decode("utf-8"))
                except Exception as e:
                    raise RuntimeError(f"Failed to fetch package definition from {def_url}: {e}")
            else:
                local_def = Path(repo_url[7:] if repo_url.startswith("file://") else repo_url) / def_path
                if not local_def.exists():
                    raise FileNotFoundError(f"Local package definition file not found: {local_def}")
                with open(local_def, "r", encoding="utf-8") as f:
                    data = json.load(f)

            validate_package_definition(data)
            self.config.save_json_atomic(cached_def_file, data)
            return data

        return None
