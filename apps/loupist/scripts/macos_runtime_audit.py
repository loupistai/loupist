#!/usr/bin/env python3
"""Report Mach-O dependencies and observed signing facts without executing the runtime."""

import argparse
import json
from pathlib import Path

from macos_release_signing import entitlements, external_libraries, macho_files, run


def main() -> None:

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    root = parser.parse_args().root
    files = macho_files(root)
    if not files:
        raise ValueError(f"no Mach-O files found in {root}")
    for path in files:
        signature = run("codesign", "-dvv", str(path), check=False)
        lines = signature.stderr.splitlines()
        print(
            json.dumps(
                {
                    "path": str(path.relative_to(root)),
                    "format": run("file", "-b", str(path)).stdout.strip(),
                    "external_libraries": external_libraries(path),
                    "signature": [
                        line
                        for line in lines
                        if line.startswith(
                            (
                                "Authority=",
                                "TeamIdentifier=",
                                "Timestamp=",
                                "CodeDirectory ",
                                "Signature=",
                            )
                        )
                    ],
                    "entitlements": entitlements(path)
                    if signature.returncode == 0
                    else None,
                },
                sort_keys=True,
            ),
            flush=True,
        )


if __name__ == "__main__":
    main()
