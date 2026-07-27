#!/usr/bin/env bash
# Fetch the Tesseract English model the bundler ships inside every Linux bundle.
#
# The AppImage has no package manager to declare a dependency to, and the deb
# and rpm cannot name one model across distributions that package different
# variants, so all three carry this file instead.
#
# Which model matters. The LSTM-only variants (tessdata_fast, tessdata_best)
# read the reward screen fine but come back empty on a riven card — its text is
# small and low-contrast, and only the main repository's model, which still
# carries the legacy engine alongside the LSTM one, transcribes it. That model
# is also what this project's OCR was measured against, so the checksum below
# pins the exact file rather than tracking the branch.
set -euo pipefail

readonly URL="https://raw.githubusercontent.com/tesseract-ocr/tessdata/main/eng.traineddata"
readonly SHA256="daa0c97d651c19fba3b25e81317cd697e9908c8208090c94c3905381c23fc047"

dest_dir="$(dirname "$0")/../src-tauri/tessdata"
dest="${dest_dir}/eng.traineddata"

# Nothing to do when the pinned file is already in place — this runs on every
# bundle, and the download is 23 MB.
if [ -f "$dest" ] && echo "${SHA256}  ${dest}" | sha256sum --check --status; then
    exit 0
fi

mkdir -p "$dest_dir"
echo "fetching eng.traineddata (23 MB) for the bundle…" >&2
curl --fail --location --silent --show-error --output "${dest}.part" "$URL"

# Verify before moving into place: a truncated or substituted model would fail
# at OCR time inside a packaged app, where there is nothing left to debug with.
if echo "${SHA256}  ${dest}.part" | sha256sum --check --status; then
    mv "${dest}.part" "$dest"
else
    rm -f "${dest}.part"
    echo "eng.traineddata checksum mismatch — refusing to bundle it" >&2
    exit 1
fi
