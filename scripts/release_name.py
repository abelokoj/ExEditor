"""ExcellentBible-compatible release identity, shared by all GitHub packages."""
import argparse
from datetime import date, datetime
import json
import os
from pathlib import Path
import re
import tomllib
import urllib.request
from zoneinfo import ZoneInfo


def make_release(version, day, number):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", version):
        raise ValueError("The package version must be a safe semantic version.")
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", day) or date.fromisoformat(day).isoformat() != day:
        raise ValueError("The release date must be valid YYYY-MM-DD.")
    if not re.fullmatch(r"[1-9][0-9]*", number) or int(number) > 2147483647:
        raise ValueError("The release number must be a positive 32-bit integer.")
    short_day = date.fromisoformat(day).strftime("%y-%m-%d")
    tag = f"v{short_day}.{number}-v{version}"
    return {"version": version, "release_date": day, "release_number": number,
            "release_tag": tag, "release_base": f"ExEditor-{tag}"}



def read_history(project):
    history = json.loads((project / "release-history.json").read_text(encoding="utf-8"))
    if history.get("format") != 1:
        raise ValueError("The release-history format is unsupported.")
    records = history["releases"]
    if [r["number"] for r in records] != list(range(1, len(records) + 1)):
        raise ValueError("Release history must retain consecutive lifetime numbers starting at 1.")
    runs = [r["workflow_run"] for r in records]
    if len(set(runs)) != len(runs) or any(not isinstance(r, int) or r < 1 for r in runs):
        raise ValueError("Release history contains invalid or duplicate workflow identities.")
    return records


def next_release_number(records, completed_runs):
    recorded = {r["workflow_run"] for r in records}
    return len(records) + len(set(completed_runs) - recorded) + 1


def successful_workflow_runs():
    repository = os.environ["GITHUB_REPOSITORY"]
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "ExEditor-release-numbering",
               "Authorization": "Bearer " + os.environ["GITHUB_TOKEN"]}
    runs = []
    page = 1
    while True:
        url = f"https://api.github.com/repos/{repository}/actions/workflows/build.yml/runs?status=success&per_page=100&page={page}"
        with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as response:
            batch = json.load(response)["workflow_runs"]
        runs.extend(r["id"] for r in batch)
        if len(batch) < 100:
            return runs
        page += 1


def release_metadata(project):
    version = tomllib.loads((project / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    day = os.environ.get("EXEDITOR_RELEASE_DATE")
    if not day:
        day = datetime.now(ZoneInfo("America/New_York")).date().isoformat()
    number = os.environ.get("EXEDITOR_RELEASE_NUMBER")
    if not number:
        records = read_history(project)
        current = int(os.environ["GITHUB_RUN_ID"])
        previous = next((r["number"] for r in records if r["workflow_run"] == current), None)
        number = str(previous or next_release_number(records, successful_workflow_runs()))
    release = make_release(version, day, number)
    for key in ("release_tag", "release_base"):
        supplied = os.environ.get("EXEDITOR_" + key.upper())
        if supplied and supplied != release[key]:
            raise ValueError(f"The shared {key} does not match Cargo and the release date/number.")
    return release


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--github-output", action="store_true")
    args = parser.parse_args()
    release = release_metadata(Path(__file__).resolve().parent.parent)
    if args.github_output:
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
            output.write("".join(f"{key}={value}\n" for key, value in release.items()))
    print(json.dumps(release))
