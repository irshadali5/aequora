#!/usr/bin/env python3
"""
Aequora Workspace Publish Orchestrator for Crates.io
Publishes workspace crates in topological dependency order across 8 layers.
Handles crates.io rate limiting, index verification, and resumption.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections import defaultdict, deque

WORKSPACE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def load_workspace_manifest():
    cargo_path = os.path.join(WORKSPACE_ROOT, "Cargo.toml")
    with open(cargo_path, "r", encoding="utf-8") as f:
        content = f.read()

    # Simple TOML parser for members list
    members = []
    in_members = False
    for line in content.splitlines():
        line = line.strip()
        if line.startswith("members = ["):
            in_members = True
            continue
        if in_members:
            if line.startswith("]"):
                break
            member = line.strip('", ')
            if member:
                members.append(member)
    return members


def load_crate_manifest(rel_path):
    cargo_path = os.path.join(WORKSPACE_ROOT, rel_path, "Cargo.toml")
    if not os.path.exists(cargo_path):
        return None

    with open(cargo_path, "r", encoding="utf-8") as f:
        lines = f.readlines()

    name = None
    publish = True
    deps = set()
    current_section = None

    for raw_line in lines:
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            current_section = line.strip("[]")
            continue

        if current_section == "package":
            if line.startswith("name ="):
                name = line.split("=")[1].strip(' "')
            elif line.startswith("publish = false"):
                publish = False

        elif current_section in ("dependencies", "build-dependencies"):
            if "=" in line:
                dep_name = line.split("=")[0].strip()
                val = line.split("=", 1)[1].strip()
                if "path =" in val or "path=" in val:
                    # check if renamed
                    pkg_match = re.search(r'package\s*=\s*"([^"]+)"', val)
                    target = pkg_match.group(1) if pkg_match else dep_name
                    deps.add(target)

    return {
        "name": name or os.path.basename(rel_path),
        "path": rel_path,
        "publish": publish,
        "deps": deps,
    }


def compute_topological_layers(crates_info):
    publishable = {k: v for k, v in crates_info.items() if v["publish"]}
    crate_deps = {}
    for name, info in publishable.items():
        # filter deps to only internal publishable crates
        crate_deps[name] = info["deps"].intersection(publishable.keys())

    in_degree = {k: 0 for k in crate_deps}
    adj = defaultdict(list)
    for node, deps in crate_deps.items():
        for d in deps:
            adj[d].append(node)
            in_degree[node] += 1

    queue = deque([k for k, v in in_degree.items() if v == 0])
    layers = []
    published = set()

    while queue:
        layer = sorted(list(queue))
        layers.append(layer)
        next_queue = deque()
        for node in layer:
            published.add(node)
            for neighbor in adj[node]:
                in_degree[neighbor] -= 1
                if in_degree[neighbor] == 0:
                    next_queue.append(neighbor)
        queue = next_queue

    remaining = set(publishable.keys()) - published
    if remaining:
        raise RuntimeError(f"Cyclic dependency detected among: {remaining}")

    return layers


def check_crates_io_version(crate_name, target_version="0.1.0"):
    url = f"https://crates.io/api/v1/crates/{crate_name}"
    req = urllib.request.Request(
        url,
        headers={"User-Agent": "aequora-publish-orchestrator (github.com/irshadali5/aequora)"},
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
            versions = [v.get("num") for v in data.get("versions", [])]
            return target_version in versions
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return False
        # If rate limited or other error, return None (unknown)
        return None
    except Exception:
        return None


def publish_crate(crate_name, allow_dirty=False, dry_run=False, no_verify=False):
    cmd = ["cargo", "publish", "-p", crate_name]
    if allow_dirty:
        cmd.append("--allow-dirty")
    if dry_run:
        cmd.append("--dry-run")
    if no_verify:
        cmd.append("--no-verify")

    print(f"\n🚀 Running: {' '.join(cmd)}")
    res = subprocess.run(cmd, cwd=WORKSPACE_ROOT, capture_output=True, text=True)
    return res.returncode == 0, res.stdout, res.stderr


def main():
    parser = argparse.ArgumentParser(description="Publish Aequora workspace crates to crates.io")
    parser.add_argument("--dry-run", action="store_true", help="Perform a dry-run without uploading")
    parser.add_argument("--allow-dirty", action="store_true", help="Allow uncommitted local changes")
    parser.add_argument("--no-verify", action="store_true", help="Pass --no-verify to cargo publish")
    parser.add_argument("--layer", type=int, help="Publish only the specified layer number (1-8)")
    parser.add_argument("--crate", type=str, help="Publish only the specified crate")
    parser.add_argument("--delay", type=int, default=15, help="Delay in seconds between crates (default: 15s)")
    parser.add_argument("--skip-api-check", action="store_true", help="Skip crates.io API check")
    args = parser.parse_args()

    members = load_workspace_manifest()
    crates_info = {}
    for m in members:
        info = load_crate_manifest(m)
        if info:
            crates_info[info["name"]] = info

    layers = compute_topological_layers(crates_info)
    total_publishable = sum(len(l) for l in layers)

    print("=" * 70)
    print("Aequora Workspace Crates.io Publisher")
    print("=" * 70)
    print(f"Total workspace crates:   {len(members)}")
    print(f"Publishable crates:       {total_publishable}")
    print(f"Topological layers:       {len(layers)}")
    print("=" * 70)

    if args.crate:
        target = args.crate
        if target not in crates_info:
            print(f"Error: Crate '{target}' not found in workspace.")
            sys.exit(1)
        if not crates_info[target]["publish"]:
            print(f"Error: Crate '{target}' has publish = false.")
            sys.exit(1)

        print(f"\nProcessing single crate: {target}")
        if not args.skip_api_check:
            is_pub = check_crates_io_version(target)
            if is_pub:
                print(f"✅ {target} v0.1.0 is already published on crates.io.")
                return

        success, out, err = publish_crate(target, allow_dirty=args.allow_dirty, dry_run=args.dry_run, no_verify=args.no_verify)
        if success:
            print(f"✅ Successfully published {target}!")
            if out:
                print(out)
        else:
            print(f"❌ Failed to publish {target}:\n{err}")
            sys.exit(1)
        return

    # Print overview of layers
    for i, layer in enumerate(layers, 1):
        print(f"\n--- Layer {i}/{len(layers)} ({len(layer)} crates) ---")
        print(", ".join(layer))

    if args.dry_run:
        print("\n[DRY RUN MODE] Checking crates.io status for all crates...")

    for i, layer in enumerate(layers, 1):
        if args.layer and args.layer != i:
            continue

        print(f"\n{'='*70}")
        print(f"Executing Layer {i}/{len(layers)} ({len(layer)} crates)")
        print(f"{'='*70}")

        for crate_name in layer:
            print(f"\n📦 Checking {crate_name}...")
            if not args.skip_api_check:
                is_pub = check_crates_io_version(crate_name)
                if is_pub:
                    print(f"  ⏩ {crate_name} v0.1.0 is already published on crates.io. Skipping.")
                    continue
                elif is_pub is False:
                    print(f"  🔍 {crate_name} not yet found on crates.io.")

            if args.dry_run:
                success, out, err = publish_crate(crate_name, allow_dirty=args.allow_dirty, dry_run=True, no_verify=args.no_verify)
                if success:
                    print(f"  [DRY RUN OK] {crate_name}")
                else:
                    print(f"  [DRY RUN FAILED] {crate_name}:\n{err}")
                continue

            # Live publishing with rate-limit retry
            max_retries = 3
            for attempt in range(1, max_retries + 1):
                success, out, err = publish_crate(crate_name, allow_dirty=args.allow_dirty, dry_run=False, no_verify=args.no_verify)
                if success:
                    print(f"  ✅ Published {crate_name} v0.1.0")
                    if out:
                        print(f"     {out.strip()}")
                    break
                else:
                    if "already exists" in err:
                        print(f"  ✅ {crate_name} v0.1.0 is already published on crates.io.")
                        break
                    elif "429" in err or "rate limit" in err.lower() or "too many requests" in err.lower():
                        print(f"  ⏳ Hit crates.io rate limit on {crate_name} (attempt {attempt}/{max_retries}).")
                        print("  Waiting 10 minutes (600s) for rate limit bucket replenishment...")
                        for sec_left in range(600, 0, -60):
                            print(f"    {sec_left} seconds remaining...")
                            time.sleep(60)
                    else:
                        print(f"  ❌ Publish failed for {crate_name}:\n{err}")
                        sys.exit(1)

            if args.delay > 0 and not args.dry_run:
                print(f"  ⏱️  Waiting {args.delay}s for crates.io index propagation...")
                time.sleep(args.delay)

    print("\n" + "=" * 70)
    print("✨ Publish workflow completed successfully!")
    print("=" * 70)


if __name__ == "__main__":
    main()
