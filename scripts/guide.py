#!/usr/bin/env python3
"""Find or read a section of Loupist's developer guide. Uses Git and Python 3."""

import argparse
import os
from functools import lru_cache
from pathlib import Path
import re
import subprocess
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
EXTERNAL_GUIDES = {
    Path("AGENTS.md"),
    Path("CLAUDE.md"),
    Path("apps/loupist/e2e/README.md"),
    Path("apps/loupist/evals/README.md"),
}


def git_files():
    data = subprocess.check_output(
        ["git", "ls-files", "-coz", "--exclude-standard"], cwd=ROOT
    )
    return {Path(os.fsdecode(name)) for name in data.split(b"\0") if name}


@lru_cache(maxsize=None)
def lines(path):
    return (ROOT / path).read_text(encoding="utf-8").splitlines()


def prose(path):
    """Markdown outside fenced samples, with original line numbers."""
    fence = ""
    for i, line in enumerate(lines(path)):
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})", line)
        if marker:
            if not fence:
                fence = marker[1]
            elif marker[1].startswith(fence):
                fence = ""
        elif not fence:
            yield i, line


@lru_cache(maxsize=None)
def headings(path):
    result = {}
    for i, line in prose(path):
        match = re.match(r"^(#{1,6})\s+(.+?)\s*#*\s*$", line)
        if match:
            title = match[2]
            base = re.sub(r"[^\w -]", "", title.lower()).replace(" ", "-")
            key = base
            n = 0
            while key in result:
                n += 1
                key = f"{base}-{n}"
            result[key] = (i, len(match[1]), title)
    return result


def find_sections(guides, query):
    terms = query.lower().replace("-", " ").split()
    found = []
    body_matches = []
    for path in guides:
        sections = list(headings(path).items())
        for n, (anchor, (start, level, title)) in enumerate(sections):
            label = title.lower().replace("-", " ")
            haystack = f"{path.as_posix().lower()} {label}".replace("-", " ")
            ref = f"{path.as_posix()}#{anchor}"
            index = path.parent == Path("docs")
            if all(term in haystack for term in terms):
                rank = (-sum(term in label for term in terms), index, level)
                found.append((rank, ref))
            else:
                # Attribute body hits to the nearest heading, without returning the page.
                end = sections[n + 1][1][0] if n + 1 < len(sections) else None
                body = " ".join(lines(path)[start:end]).lower()
                if all(term in body for term in terms):
                    body_matches.append(((0, index, level), ref))
    hits = found or body_matches
    if not hits:
        raise SystemExit("No matching section. Search the code with rg -n.")
    for _, ref in sorted(hits)[:12]:
        print(ref)


def read_section(guides, ref):
    name, _, anchor = ref.partition("#")
    path = Path(name)
    if path not in guides or anchor not in headings(path):
        raise SystemExit(f"Unknown guide section: {ref}")
    start, depth, _ = headings(path)[anchor]
    end = next(
        (i for i, level, _ in headings(path).values() if i > start and level <= depth),
        None,
    )
    print("\n".join(lines(path)[start:end]).rstrip())


def check_links(guides, files):
    # Validate inline repository links used by the guide, including same-page anchors.
    # Ignored local reports can exist on this machine and still be missing in a clone.
    directories = {directory for file in files for directory in file.parents}
    visible = files | directories
    errors = []
    for path in sorted(guides):
        for i, line in prose(path):
            destinations = re.findall(r"\]\(([^()\s]+)\)", re.sub(r"`[^`]*`", "", line))
            if path == Path("CLAUDE.md") and line.startswith("@"):
                destinations.append(line[1:].strip())
            for dest in destinations:
                url = urlsplit(dest)
                if url.scheme or url.netloc:
                    continue
                local = (
                    Path(os.path.normpath(path.parent / unquote(url.path)))
                    if url.path
                    else path
                )
                target = (ROOT / local).resolve()
                valid = (
                    local in visible and target.is_relative_to(ROOT) and target.exists()
                )
                if valid:
                    rel = target.relative_to(ROOT)
                    valid = rel in visible
                    if valid and url.fragment and rel.suffix == ".md":
                        valid = unquote(url.fragment) in headings(rel)
                if not valid:
                    errors.append(f"{path}:{i + 1}: {dest}")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"Checked local Markdown links in {len(guides)} guides.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "topic", nargs="*", help="words to find, or a path#heading returned by search"
    )
    parser.add_argument(
        "--check", action="store_true", help="check local guide links and anchors"
    )
    args = parser.parse_args()
    files = git_files()
    guides = {
        p
        for p in files
        if p.suffix == ".md" and p.parts[0] == "docs"
    } | EXTERNAL_GUIDES
    # Do not follow a guide symlink into another checkout or private host files.
    readable = {
        p
        for p in guides
        if p in files and (ROOT / p).is_file() and (ROOT / p).resolve() == ROOT / p
    }
    if args.check and readable != guides:
        invalid = sorted(str(p) for p in guides - readable)
        raise SystemExit("Missing or linked guide: " + ", ".join(invalid))
    guides = readable
    if args.check:
        check_links(guides, files)
    elif len(args.topic) == 1 and "#" in args.topic[0]:
        read_section(guides, args.topic[0])
    elif args.topic:
        find_sections(guides, " ".join(args.topic))
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
