#!/usr/bin/env python3
"""Recipe CI: installs recipes headlessly and checks they worked.

  recipe_ci.py matrix [--changed BASE]   job matrix JSON for GitHub Actions
  recipe_ci.py run RECIPE RUNTIME        install one recipe with one runtime

`run` needs the `vinter` binary on PATH (or $VINTER). It installs RUNTIME,
sets the recipe up in a fresh bottle, checks every program's .exe exists,
and runs the log doctor over the bottle's logs. Error findings fail the run.
"""

import json
import os
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
RECIPES = ROOT / "recipes"
MANIFEST = ROOT / "crates" / "vinter-core" / "runtimes.json"
# Changes here affect how every recipe runs.
ENGINE = ("crates/vinter-core/src/recipe/", "crates/vinter-cli/", "scripts/recipe_ci.py")
RUNNERS = {"macos": "macos-14", "linux": "ubuntu-24.04"}


def recipes():
    """Recipes CI can install: user-provided installers cannot be fetched."""
    out = {}
    for path in sorted(RECIPES.glob("*.toml")):
        recipe = tomllib.loads(path.read_text())
        kind = recipe.get("installer", {}).get("kind")
        if kind != "user_provided":
            out[recipe["id"]] = path
    return out


def runtimes(newest_only):
    """Runtimes per OS, in manifest order. newest_only: the last stable one."""
    manifest = json.loads(MANIFEST.read_text())
    by_os = {}
    for r in manifest["runtimes"]:
        if r["os"] in RUNNERS:
            by_os.setdefault(r["os"], []).append(r)
    if newest_only:
        return {
            os_: [[r for r in rs if r["flavor"] == "stable"][-1]]
            for os_, rs in by_os.items()
        }
    return by_os


def changed_files(base):
    diff = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...HEAD"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    return diff.stdout.split()


def matrix(base):
    all_recipes = recipes()
    if base:
        # A pull request: the recipes it touches, with the newest runtime.
        # A runtime manifest change tests every recipe with every runtime.
        changed = changed_files(base)
        if any(f.endswith("runtimes.json") for f in changed):
            wanted, newest_only = list(all_recipes), False
        elif any(f.startswith(ENGINE) for f in changed):
            # How recipes run changed: every recipe, newest runtime.
            wanted, newest_only = list(all_recipes), True
        else:
            touched = {pathlib.Path(f).resolve() for f in changed}
            wanted = [i for i, p in all_recipes.items() if p.resolve() in touched]
            newest_only = True
    else:
        wanted, newest_only = list(all_recipes), False
    include = [
        {"os": RUNNERS[os_], "runtime": rt["id"], "recipe": recipe}
        for os_, rts in runtimes(newest_only).items()
        for rt in rts
        for recipe in wanted
    ]
    return {"include": include}


def vinter(*args):
    exe = os.environ.get("VINTER", "vinter")
    proc = subprocess.run([exe, *args, "--json"], capture_output=True, text=True)
    sys.stderr.write(proc.stderr)
    try:
        value = json.loads(proc.stdout)
    except json.JSONDecodeError:
        raise SystemExit(f"vinter {' '.join(args)} printed no JSON:\n{proc.stdout}")
    if isinstance(value, dict) and "error" in value:
        raise SystemExit(f"vinter {' '.join(args)}: {value['error']}")
    return value


def drive_c_path(drive_c, windows_path):
    """C:\\Program Files\\App\\app.exe -> <drive_c>/Program Files/App/app.exe"""
    rest = windows_path[3:] if windows_path[:3].upper() == "C:\\" else windows_path
    return pathlib.Path(drive_c, *rest.split("\\"))


def run(recipe_id, runtime_id):
    problems = []
    installed = {r["id"] for r in vinter("runtime", "list")["installed"]}
    if runtime_id not in installed:
        print(f"Installing runtime {runtime_id}", flush=True)
        vinter("runtime", "install", runtime_id)

    print(f"Setting up {recipe_id}", flush=True)
    result = vinter("recipe", "apply", recipe_id, "--name", f"ci-{recipe_id}", "--accept-downloads")
    bottle = result["bottle"]
    if bottle["runtime"] != runtime_id:
        problems.append(f"used runtime {bottle['runtime']}, not {runtime_id}")

    drive_c = vinter("bottle", "drive-c", bottle["id"])["path"]
    if not bottle["programs"]:
        problems.append("no programs were added")
    for program in bottle["programs"]:
        exe = drive_c_path(drive_c, program["exe"])
        status = "ok" if exe.is_file() else "MISSING"
        print(f"  {status:7} {program['name']}: {program['exe']}")
        if not exe.is_file():
            problems.append(f"{program['name']}: {program['exe']} was not installed")

    for log in vinter("bottle", "logs", bottle["id"]):
        for f in vinter("bottle", "doctor", bottle["id"], log["id"])["findings"]:
            print(f"  doctor  [{f['severity']}] {log['label']}: {f['title']}")
            if f["severity"] == "error":
                problems.append(f"log doctor: {f['title']} ({log['label']})")

    summary = {"recipe": recipe_id, "runtime": runtime_id, "ok": not problems, "problems": problems}
    print(json.dumps(summary, indent=2))
    return 0 if not problems else 1


def main(argv):
    if argv[:1] == ["matrix"]:
        base = argv[2] if argv[1:2] == ["--changed"] and len(argv) > 2 else None
        print(json.dumps(matrix(base)))
        return 0
    if argv[:1] == ["run"] and len(argv) == 3:
        return run(argv[1], argv[2])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
