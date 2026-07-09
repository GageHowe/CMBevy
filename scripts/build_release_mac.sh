#!/usr/bin/env bash
set -euo pipefail

"$(dirname "$0")/build_release_unix.sh" libsteam_api.dylib libfmod.dylib libfmodstudio.dylib
