"""Package the voice-input extension with the shared dictation implementation."""

import argparse
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", type=Path, default=repo / "build/extensions/dictation-words.sayso")
    output = parser.parse_args().output

    recipe = repo / "examples/dictation-words"
    dictation = repo / "core/plugins/dictation"
    output.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(output, "w", compression=ZIP_DEFLATED) as archive:
        archive.write(recipe / "extension.json", "extension.json")
        archive.write(recipe / "main.js", "main.js")
        archive.write(dictation / "main.js", "dictation.js")
        for module in ("prompt.js", "languages.js", "transcript.js"):
            archive.write(dictation / module, module)
    print(output.resolve())


if __name__ == "__main__":
    main()
