#!/usr/bin/env python3
"""Generate and check the ACP agent catalog (docs/acp-integration/TRD.md §4.3.13).

    python3 scripts/acp_catalog.py update   # maintainer, needs network
    python3 scripts/acp_catalog.py check    # offline, CI
    python3 scripts/acp_catalog.py verify-node  # re-check Node hashes and signature

`update` reads catalog/pins.toml and writes catalog/catalog.toml,
catalog/locks/<id>-<version>.json and ../locks.rs. Standard library only.
npm lockfiles need npm >= 11.11 (lockfile `libc` fields); when the npm on PATH
is older, the pinned Node runtime for this machine is downloaded into a
temporary directory, verified against SHASUMS256.txt, and its npm is used.
"""
import base64
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import urllib.request
import zipfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INSTALL = ROOT / "crates/pocket-codex-host-svc/src/acp/install"
CATALOG_DIR = INSTALL / "catalog"
PINS = CATALOG_DIR / "pins.toml"
CATALOG = CATALOG_DIR / "catalog.toml"
LOCKS_DIR = CATALOG_DIR / "locks"
LOCKS_RS = INSTALL / "locks.rs"
NPM_REGISTRY = "https://registry.npmjs.org/"
ACP_REGISTRY = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json"
# Node.js release signing keys (https://github.com/nodejs/release-keys).
NODE_KEYRING = "https://raw.githubusercontent.com/nodejs/release-keys/main/gpg/pubring.kbx"
PLATFORMS = ["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64",
             "windows-aarch64", "windows-x86_64"]
USER_AGENT = "pocket-codex-acp-catalog"


def fail(message):
    raise SystemExit(f"acp_catalog: {message}")


# ---------------------------------------------------------------- network

def fetch(url, attempts=4):
    last = None
    for attempt in range(attempts):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(request, timeout=120) as response:
                return response.read()
        except Exception as error:  # noqa: BLE001 - retried, then reported
            last = error
            time.sleep(2 * (attempt + 1))
    fail(f"GET {url}: {last}")


def download(url, dest, attempts=4):
    last = None
    for attempt in range(attempts):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            digest = hashlib.sha256()
            with urllib.request.urlopen(request, timeout=300) as response, open(dest, "wb") as out:
                while True:
                    chunk = response.read(1 << 20)
                    if not chunk:
                        break
                    digest.update(chunk)
                    out.write(chunk)
            return digest.hexdigest()
        except Exception as error:  # noqa: BLE001 - retried, then reported
            last = error
            time.sleep(2 * (attempt + 1))
    fail(f"download {url}: {last}")


def sri_from_hex(hex_digest):
    return "sha256-" + base64.b64encode(bytes.fromhex(hex_digest)).decode()


# ------------------------------------------------------------------- node

def node_targets(pin):
    version = pin["version"]
    base = f"https://nodejs.org/dist/v{version}/"
    sums_text = fetch(base + "SHASUMS256.txt").decode()
    verify_signature(base, sums_text)
    sums = {}
    for line in sums_text.splitlines():
        parts = line.split()
        if len(parts) == 2:
            sums[parts[1]] = parts[0]
    targets = {}
    for key, suffix in pin["files"].items():
        name = f"node-v{version}-{suffix}"
        if name not in sums:
            fail(f"{name} is missing from SHASUMS256.txt")
        root = re.sub(r"\.(tar\.gz|zip)$", "", name)
        targets[key] = {"url": base + name, "integrity": sri_from_hex(sums[name]), "root": root}
    return targets, sums


def verify_signature(base, sums_text):
    """Check SHASUMS256.txt.sig with gpgv and the Node.js release keyring."""
    gpgv = shutil.which("gpgv")
    if not gpgv:
        print("warning: gpgv not found; SHASUMS256.txt signature not verified")
        return
    with tempfile.TemporaryDirectory(dir=os.environ.get("ACP_CATALOG_TMP")) as tmp:
        tmp = Path(tmp)
        (tmp / "SHASUMS256.txt").write_text(sums_text)
        (tmp / "SHASUMS256.txt.sig").write_bytes(fetch(base + "SHASUMS256.txt.sig"))
        (tmp / "pubring.kbx").write_bytes(fetch(NODE_KEYRING))
        result = subprocess.run([gpgv, "--keyring", str(tmp / "pubring.kbx"),
                                 str(tmp / "SHASUMS256.txt.sig"), str(tmp / "SHASUMS256.txt")],
                                capture_output=True, text=True)
        if result.returncode != 0:
            fail(f"SHASUMS256.txt signature check failed:\n{result.stderr}")
        print("SHASUMS256.txt signature verified with gpgv")


def verify_node():
    """Re-check the catalog's Node hashes against the signed SHASUMS256.txt."""
    catalog = tomllib.loads(CATALOG.read_text())
    node = catalog.get("node")
    if not node:
        print("no [node] in catalog.toml")
        return
    base = f"https://nodejs.org/dist/v{node['version']}/"
    sums_text = fetch(base + "SHASUMS256.txt").decode()
    verify_signature(base, sums_text)
    sums = {parts[1]: parts[0] for parts in (l.split() for l in sums_text.splitlines()) if len(parts) == 2}
    for key, target in node["targets"].items():
        name = target["url"].rsplit("/", 1)[-1]
        if sums.get(name) is None or sri_from_hex(sums[name]) != target["integrity"]:
            fail(f"node {key}: integrity does not match SHASUMS256.txt")
    print(f"Node {node['version']} hashes match the signed SHASUMS256.txt")


def local_node(pin, sums, workdir):
    """Download the pinned Node for this machine and return its npm command."""
    machine = platform.machine().lower()
    arch = "arm64" if machine in ("arm64", "aarch64") else "x64"
    system = {"Darwin": "darwin", "Linux": "linux", "Windows": "win"}.get(platform.system())
    if system is None:
        fail(f"unsupported maintainer platform {platform.system()}")
    ext = "zip" if system == "win" else "tar.gz"
    name = f"node-v{pin['version']}-{system}-{arch}.{ext}"
    archive = workdir / name
    digest = download(f"https://nodejs.org/dist/v{pin['version']}/{name}", archive)
    if sums.get(name) != digest:
        fail(f"{name}: sha256 mismatch")
    if ext == "zip":
        with zipfile.ZipFile(archive) as zf:
            zf.extractall(workdir)
    else:
        with tarfile.open(archive) as tf:
            tf.extractall(workdir, filter="data")
    root = workdir / name.removesuffix("." + ext)
    if system == "win":
        return [str(root / "node.exe"), str(root / "node_modules/npm/bin/npm-cli.js")]
    return [str(root / "bin/node"), str(root / "lib/node_modules/npm/bin/npm-cli.js")]


def npm_command(pin, sums, workdir):
    npm = shutil.which("npm")
    if npm:
        try:
            version = subprocess.check_output([npm, "--version"], text=True).strip()
            major, minor = (int(x) for x in version.split(".")[:2])
            if (major, minor) >= (11, 11):
                return [npm]
            print(f"npm {version} on PATH is older than 11.11; using the pinned Node's npm")
        except (subprocess.CalledProcessError, ValueError):
            pass
    return local_node(pin, sums, workdir)


# ------------------------------------------------------------------ agents

def package_json(agent_id, package, version):
    return {"name": f"pcx-{agent_id}", "private": True, "dependencies": {package: version}}


def lock_for(npm, agent, release, workdir):
    target = workdir / f"lock-{agent['id']}-{release['version']}"
    target.mkdir()
    manifest = package_json(agent["id"], release["package"], release["version"])
    (target / "package.json").write_text(json.dumps(manifest))
    subprocess.check_call(npm + ["install", "--package-lock-only", "--ignore-scripts", "--save-exact",
                                 "--no-audit", "--no-fund", f"--registry={NPM_REGISTRY}"], cwd=target)
    lock = json.loads((target / "package-lock.json").read_text())
    check_lock_data(lock, f"{agent['id']}-{release['version']}")
    return lock


def registry_sha(registry, registry_id, version, key):
    for entry in registry.get("agents", []):
        if entry.get("id") == registry_id and entry.get("version") == version:
            return entry.get("distribution", {}).get("binary", {}).get(key.removesuffix("-baseline"), {}).get("sha256")
    return None


def find_member(archive, binary, windows):
    wanted = binary + (".exe" if windows else "")
    if str(archive).endswith(".zip"):
        with zipfile.ZipFile(archive) as zf:
            names = zf.namelist()
    else:
        with tarfile.open(archive) as tf:
            names = tf.getnames()
    matches = sorted((n for n in names if n.rstrip("/").split("/")[-1] == wanted), key=len)
    if not matches:
        fail(f"{archive.name}: no `{wanted}` inside")
    return matches[0].lstrip("./")


def github_targets(release, registry, registry_id, workdir):
    api = f"https://api.github.com/repos/{release['repo']}/releases/tags/{release['tag']}"
    assets = {a["name"]: a for a in json.loads(fetch(api))["assets"]}
    targets = {}
    for key, name in release["assets"].items():
        asset = assets.get(name)
        if asset is None:
            print(f"warning: {release['tag']} has no {name}; {key} is not supported")
            continue
        path = workdir / name
        digest = download(asset["browser_download_url"], path)
        api_digest = (asset.get("digest") or "").removeprefix("sha256:")
        if api_digest and api_digest != digest:
            fail(f"{name}: sha256 differs from the GitHub digest")
        listed = registry_sha(registry, registry_id, release["version"], key) if registry_id else None
        if listed and not key.endswith("-baseline") and listed.lower() != digest:
            fail(f"{name}: sha256 differs from the ACP registry")
        cmd = find_member(path, release.get("binary", "opencode"), key.startswith("windows"))
        targets[key] = {"url": asset["browser_download_url"], "integrity": sri_from_hex(digest), "cmd": cmd}
        path.unlink()
    return targets


def npm_platform_targets(release):
    targets = {}
    for key, suffix in release["packages"].items():
        package = release["package_prefix"] + suffix
        meta_url = NPM_REGISTRY + package.replace("/", "%2F") + "/" + release["version"]
        meta = json.loads(fetch(meta_url))
        dist = meta["dist"]
        exe = ".exe" if key.startswith("windows") else ""
        targets[key] = {"url": dist["tarball"], "integrity": dist["integrity"],
                        "cmd": f"package/bin/{release.get('binary', 'opencode')}{exe}"}
    return targets


# ------------------------------------------------------------------ output

def q(value):
    return json.dumps(value, ensure_ascii=False)


def inline(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, str):
        return q(value)
    if isinstance(value, list):
        return "[" + ", ".join(inline(v) for v in value) + "]"
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{k} = {inline(v)}" for k, v in value.items()) + " }"
    fail(f"cannot write {value!r}")


AGENT_KEYS = ["id", "name", "description", "registry_id", "approx_size_mb", "engine_override",
              "external_engine"]
RELEASE_KEYS = ["version", "engine_range", "kind", "package", "entry", "lock", "omit", "platforms",
                "args", "env", "random_env", "data_family", "conditional_args"]


def render(catalog):
    out = ["# Generated by scripts/acp_catalog.py from pins.toml; do not edit by hand.",
           "schema = 1", f"generated_at = {q(catalog['generated_at'])}", ""]
    node = catalog.get("node")
    if node:
        out += ["[node]", f"version = {q(node['version'])}"]
        for key, target in node["targets"].items():
            out.append(f"[node.targets.{key}]")
            out += [f"{k} = {inline(v)}" for k, v in target.items()]
        out.append("")
    if not catalog["agents"]:
        out.append("agents = []")
    for agent in catalog["agents"]:
        out.append("[[agents]]")
        out += [f"{k} = {inline(agent[k])}" for k in AGENT_KEYS if k in agent]
        for release in agent["releases"]:
            out.append("[[agents.releases]]")
            out += [f"{k} = {inline(release[k])}" for k in RELEASE_KEYS if k in release]
            for key, target in release.get("targets", {}).items():
                out.append(f"[agents.releases.targets.{key}]")
                out += [f"{k} = {inline(v)}" for k, v in target.items()]
        out.append("")
    return "\n".join(out).rstrip() + "\n"


def render_locks(names):
    lines = ["//! Generated by `scripts/acp_catalog.py`; do not edit by hand.", "//!",
             "//! Lock name → `package-lock.json` of every npm release in the catalog.", "",
             "/// Lockfiles embedded in the binary."]
    if not names:
        lines.append("pub static LOCKS: &[(&str, &str)] = &[];")
    else:
        lines.append("pub static LOCKS: &[(&str, &str)] = &[")
        for name in names:
            lines.append(f'    ("{name}", include_str!("catalog/locks/{name}.json")),')
        lines.append("];")
    return "\n".join(lines) + "\n"


def update():
    pins = tomllib.loads(PINS.read_text())
    workdir = Path(tempfile.mkdtemp(prefix="acp-catalog-", dir=os.environ.get("ACP_CATALOG_TMP")))
    try:
        targets, sums = node_targets(pins["node"])
        catalog = {"generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                   "node": {"version": pins["node"]["version"], "targets": targets}, "agents": []}
        registry = json.loads(fetch(ACP_REGISTRY))
        npm = None
        LOCKS_DIR.mkdir(parents=True, exist_ok=True)
        lock_names = []
        for pin in pins["agents"]:
            agent = {k: pin[k] for k in AGENT_KEYS if k in pin}
            agent["description"] = pin.get("description", "")
            agent["releases"] = []
            for rel in pin["releases"]:
                out = {k: rel[k] for k in RELEASE_KEYS if k in rel and k not in ("lock",)}
                if rel["kind"] == "npm":
                    if npm is None:
                        npm = npm_command(pins["node"], sums, workdir)
                    name = f"{pin['id']}-{rel['version']}"
                    lock = lock_for(npm, pin, rel, workdir)
                    (LOCKS_DIR / f"{name}.json").write_text(json.dumps(lock, indent=2) + "\n")
                    out["lock"] = name
                    lock_names.append(name)
                elif rel.get("source") == "github":
                    out["targets"] = github_targets(rel, registry, pin.get("registry_id"), workdir)
                elif rel.get("source") == "npm-platform":
                    out["targets"] = npm_platform_targets(rel)
                else:
                    fail(f"{pin['id']} {rel['version']}: unknown release source")
                agent["releases"].append(out)
                print(f"pinned {pin['id']} {rel['version']}")
            catalog["agents"].append(agent)
        for stale in LOCKS_DIR.glob("*.json"):
            if stale.stem not in lock_names:
                stale.unlink()
        CATALOG.write_text(render(catalog))
        LOCKS_RS.write_text(render_locks(sorted(lock_names)))
    finally:
        shutil.rmtree(workdir, ignore_errors=True)
    check()


# ------------------------------------------------------------------- check

def check_lock_data(lock, name):
    if lock.get("lockfileVersion") != 3:
        fail(f"{name}: lockfileVersion must be 3")
    for path, entry in lock.get("packages", {}).items():
        if path == "" or entry.get("link"):
            continue
        if not entry.get("integrity"):
            fail(f"{name}: {path} has no integrity")
        if not entry.get("resolved", "").startswith(NPM_REGISTRY):
            fail(f"{name}: {path} does not resolve from {NPM_REGISTRY}")


def check_sri(value, where):
    if not re.fullmatch(r"sha(256|512)-[A-Za-z0-9+/]+={0,2}", value or ""):
        fail(f"{where}: invalid integrity {value!r}")


def check():
    catalog = tomllib.loads(CATALOG.read_text())
    if catalog.get("schema") != 1:
        fail("catalog.toml: schema must be 1")
    if "generated_at" not in catalog:
        fail("catalog.toml: generated_at is missing")
    node = catalog.get("node")
    if node:
        for key, target in node.get("targets", {}).items():
            if not target.get("url", "").startswith("https://nodejs.org/"):
                fail(f"node {key}: bad url")
            check_sri(target.get("integrity"), f"node {key}")
            if not target.get("root"):
                fail(f"node {key}: root is missing")
    locks_rs = LOCKS_RS.read_text()
    embedded = set(re.findall(r'\("([^"]+)", include_str!\("catalog/locks/([^"]+)\.json"\)\)', locks_rs))
    if any(a != b for a, b in embedded):
        fail("locks.rs: lock name and file differ")
    embedded = {a for a, _ in embedded}
    files = {p.stem for p in LOCKS_DIR.glob("*.json")} if LOCKS_DIR.exists() else set()
    if embedded != files:
        fail(f"locks.rs and catalog/locks differ: {sorted(embedded ^ files)}")
    needs_node = False
    referenced = set()
    for agent in catalog.get("agents", []):
        for release in agent.get("releases", []):
            where = f"{agent.get('id')} {release.get('version')}"
            if release.get("kind") == "npm":
                needs_node = True
                lock = release.get("lock")
                if lock not in files:
                    fail(f"{where}: lockfile {lock} is missing")
                referenced.add(lock)
                check_lock_data(json.loads((LOCKS_DIR / f"{lock}.json").read_text()), lock)
                platforms = release.get("platforms", [])
                if not platforms or any(p not in PLATFORMS for p in platforms):
                    fail(f"{where}: bad platforms")
                if node and any(p not in node.get("targets", {}) for p in platforms):
                    fail(f"{where}: the Node runtime does not cover every platform")
            elif release.get("kind") == "archive":
                targets = release.get("targets", {})
                if not targets:
                    fail(f"{where}: no targets")
                for key, target in targets.items():
                    if key.removesuffix("-baseline") not in PLATFORMS:
                        fail(f"{where}: unknown target {key}")
                    if not target.get("url", "").startswith("https://"):
                        fail(f"{where} {key}: url must be https")
                    check_sri(target.get("integrity"), f"{where} {key}")
                    if not target.get("cmd"):
                        fail(f"{where} {key}: cmd is missing")
            else:
                fail(f"{where}: unknown kind")
    if needs_node and not node:
        fail("catalog.toml: npm agents need [node]")
    if referenced != files:
        fail(f"unused lockfiles: {sorted(files - referenced)}")
    print(f"ACP catalog ok ({len(catalog.get('agents', []))} agents, {len(files)} lockfiles).")


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "update":
        update()
    elif command == "check":
        check()
    elif command == "verify-node":
        verify_node()
    else:
        raise SystemExit(__doc__)
