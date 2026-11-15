"""ExcellentBible-compatible release identity, shared by all GitHub packages."""
import argparse
from datetime import date, datetime
import json
import os
from pathlib import Path
import re
import tomllib
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


def release_metadata(project):
    version = tomllib.loads((project / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    day = os.environ.get("EXEDITOR_RELEASE_DATE")
    if not day:
        day = datetime.now(ZoneInfo("America/New_York")).date().isoformat()
    number = os.environ.get("EXEDITOR_RELEASE_NUMBER") or os.environ.get("GITHUB_RUN_NUMBER", "")
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
